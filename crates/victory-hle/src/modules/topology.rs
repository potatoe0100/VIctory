//! Measurement (diagnostics builds): how the title looks at the CPU and
//! places its threads. Every call goes to the host unchanged.
#![allow(non_snake_case)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::c_void;
use std::sync::{Mutex, Once, OnceLock};
use std::time::Duration;

use crate::{HleModule, hle_export};

type Handle = *mut c_void;

#[link(name = "kernel32")]
unsafe extern "system" {
    #[link_name = "GetLastError"]
    fn last_error() -> u32;
    #[link_name = "SetLastError"]
    fn set_last_error(code: u32);
}

#[derive(Default)]
struct Seen {
    totals: BTreeMap<&'static str, u64>,
    values: BTreeSet<(&'static str, u64)>,
}

impl Seen {
    /// Counts a call; true the first time `value` is seen for `call`.
    fn note(&mut self, call: &'static str, value: u64) -> bool {
        *self.totals.entry(call).or_default() += 1;
        self.values.insert((call, value))
    }
}

static SEEN: Mutex<Seen> = Mutex::new(Seen {
    totals: BTreeMap::new(),
    values: BTreeSet::new(),
});

fn note(call: &'static str, value: u64) {
    // The title reads the host call's last error; logging must not change it.
    // SAFETY: this thread's last error.
    let error = unsafe { last_error() };
    let first = SEEN
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .note(call, value);
    if first {
        tracing::info!(call, value = format_args!("{value:#x}"), "topology: new value");
    }
    // The title is often killed, so totals cannot wait for its exit.
    static REPORT: Once = Once::new();
    REPORT.call_once(|| {
        let _ = std::thread::Builder::new()
            .name("topology".into())
            .spawn(report);
    });
    // SAFETY: as above.
    unsafe { set_last_error(error) };
}

fn report() {
    let mut last = BTreeMap::new();
    loop {
        std::thread::sleep(Duration::from_secs(30));
        let totals = SEEN
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .totals
            .clone();
        if totals != last {
            tracing::info!(?totals, "topology: calls so far");
            last = totals;
        }
    }
}

/// A host function of the same signature, noted after it returns.
macro_rules! forward {
    ($name:ident($($arg:ident: $ty:ty),*) -> $ret:ty, $value:expr) => {
        pub extern "system" fn $name($($arg: $ty),*) -> $ret {
            #[link(name = "kernel32")]
            unsafe extern "system" {
                fn $name($($arg: $ty),*) -> $ret;
            }
            // SAFETY: the title's own arguments to the host API of the same name.
            let result = unsafe { $name($($arg),*) };
            note(stringify!($name), $value);
            result
        }
    };
}

forward!(SetThreadAffinityMask(thread: Handle, mask: usize) -> usize, mask as u64);
forward!(SetThreadIdealProcessor(thread: Handle, processor: u32) -> u32, processor.into());
// Value: the `PROCESSOR_NUMBER` as stored (group low, number above it).
forward!(
    SetThreadIdealProcessorEx(thread: Handle, ideal: *const u32, previous: *mut u32) -> i32,
    if ideal.is_null() {
        u64::MAX
    } else {
        // SAFETY: not null, and the title passes its `PROCESSOR_NUMBER` there.
        unsafe { ideal.read_unaligned() }.into()
    }
);
// Negative priorities show sign-extended.
forward!(SetThreadPriority(thread: Handle, priority: i32) -> i32, priority as u64);
forward!(GetSystemInfo(info: *mut c_void) -> (), 0);
forward!(GetNativeSystemInfo(info: *mut c_void) -> (), 0);
forward!(GetLogicalProcessorInformation(buffer: *mut c_void, length: *mut u32) -> i32, 0);
// Value: the relationship asked for.
forward!(
    GetLogicalProcessorInformationEx(relationship: u32, buffer: *mut c_void, length: *mut u32) -> i32,
    relationship.into()
);
// Value: the creation flags.
forward!(
    CreateThread(
        security: *const c_void,
        stack: usize,
        start: *const c_void,
        parameter: *mut c_void,
        flags: u32,
        id: *mut u32
    ) -> Handle,
    flags.into()
);

type BeginThread =
    extern "system" fn(*mut c_void, u32, *const c_void, *mut c_void, u32, *mut u32) -> usize;

/// The CRT's thread creation, which reaches `CreateThread` past our imports.
pub extern "system" fn _beginthreadex(
    security: *mut c_void,
    stack: u32,
    start: *const c_void,
    argument: *mut c_void,
    flags: u32,
    id: *mut u32,
) -> usize {
    static HOST: OnceLock<BeginThread> = OnceLock::new();
    // Resolved with nothing held: the lookup takes the loader lock, which a
    // second caller may already own.
    let host = HOST.get().copied().or_else(|| {
        let address = crate::host::function("ucrtbase.dll", "_beginthreadex")?;
        // SAFETY: that export has this signature (x64 has one convention).
        Some(*HOST.get_or_init(|| unsafe { std::mem::transmute::<usize, BeginThread>(address) }))
    });
    let Some(host) = host else {
        tracing::warn!("topology: no _beginthreadex in the host");
        return 0;
    };
    let result = host(security, stack, start, argument, flags, id);
    note("_beginthreadex", flags.into());
    result
}

pub(crate) fn module() -> HleModule {
    HleModule {
        name: "topology",
        dlls: &[
            "kernel32.dll",
            "kernelbase.dll",
            "api-ms-win-core-*",
            "api-ms-win-crt-runtime-*",
        ],
        exports: vec![
            hle_export!(SetThreadAffinityMask),
            hle_export!(SetThreadIdealProcessor),
            hle_export!(SetThreadIdealProcessorEx),
            hle_export!(SetThreadPriority),
            hle_export!(GetSystemInfo),
            hle_export!(GetNativeSystemInfo),
            hle_export!(GetLogicalProcessorInformation),
            hle_export!(GetLogicalProcessorInformationEx),
            hle_export!(CreateThread),
            hle_export!(_beginthreadex),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_every_call_and_each_value_once() {
        let mut seen = Seen::default();
        assert!(seen.note("a", 1));
        assert!(!seen.note("a", 1));
        assert!(seen.note("a", 2));
        assert!(seen.note("b", 1));
        assert_eq!(seen.totals["a"], 3);
        assert_eq!(seen.totals["b"], 1);
    }

    #[test]
    fn last_error_passes_through() {
        // SAFETY: this thread's last error.
        unsafe { set_last_error(0) };
        assert_eq!(SetThreadPriority(std::ptr::null_mut(), 0), 0);
        // SAFETY: as above. 6 is `ERROR_INVALID_HANDLE`, for the null thread.
        assert_eq!(unsafe { last_error() }, 6);
    }
}
