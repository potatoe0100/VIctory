//! Measurement (diagnostics builds): how the title looks at the CPU and
//! places its threads. Every call goes to the host unchanged, except
//! affinity masks and ideal processors under `VICTORY_PIN_SHIFT`.
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
    fn GetCurrentProcess() -> Handle;
    fn GetProcessAffinityMask(process: Handle, process_mask: *mut usize, system_mask: *mut usize)
    -> i32;
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

/// Logical CPUs the process may run on (its affinity mask's count), read once.
/// shortcut: assumes the mask starts at CPU 0 with no gaps, true unless the
/// process is started with a restricted affinity; read the bits then.
fn cpu_count() -> u32 {
    static COUNT: OnceLock<u32> = OnceLock::new();
    *COUNT.get_or_init(|| {
        let (mut process, mut system) = (0usize, 0usize);
        // SAFETY: the current process's pseudo-handle and two locals.
        let ok = unsafe { GetProcessAffinityMask(GetCurrentProcess(), &mut process, &mut system) };
        if ok != 0 && process != 0 { process.count_ones() } else { 16 }
    })
}

/// Logical CPUs the masks rotate by (`VICTORY_PIN_SHIFT`, even and below the
/// CPU count, default 0): the title pins to CPUs 0-3, where Windows runs most
/// ISRs and DPCs. Odd shifts would split SMT pairs.
fn pin_shift() -> u32 {
    static SHIFT: OnceLock<u32> = OnceLock::new();
    *SHIFT.get_or_init(|| {
        let count = cpu_count();
        let asked = std::env::var("VICTORY_PIN_SHIFT").ok();
        let shift = asked.as_deref().map_or(Some(0), |text| {
            text.trim().parse().ok().filter(|n: &u32| *n < count && n % 2 == 0)
        });
        if shift.is_none() {
            tracing::warn!(
                ?asked,
                count,
                "topology: VICTORY_PIN_SHIFT is not even and below the CPU count (keeps SMT pairs), ignored"
            );
        }
        shift.unwrap_or(0)
    })
}

/// `mask` rotated by `shift` within `count` logical CPUs (left, or right to
/// undo): same count, never empty. Masks past the last CPU, or empty, pass
/// unchanged.
fn rotate(mask: usize, shift: u32, left: bool, count: u32) -> usize {
    let full = if count >= usize::BITS { usize::MAX } else { (1 << count) - 1 };
    if mask == 0 || shift == 0 || shift >= count || mask & !full != 0 {
        return mask;
    }
    let by = if left { shift } else { count - shift };
    ((mask << by) | (mask >> (count - by))) & full
}

/// `processor` moved by the pin shift (back, when not `left`) within the CPU
/// count; numbers past it (queries, errors) pass unchanged.
fn shift_processor(processor: u32, left: bool) -> u32 {
    let (shift, count) = (pin_shift(), cpu_count());
    if shift == 0 || processor >= count {
        processor
    } else if left {
        (processor + shift) % count
    } else {
        (processor + count - shift) % count
    }
}

/// A stored `PROCESSOR_NUMBER` (group low, number above it) with its number
/// shifted; group 0 only.
fn shift_processor_number(stored: u32, left: bool) -> u32 {
    if stored & 0xffff != 0 {
        return stored;
    }
    let number = shift_processor((stored >> 16) & 0xff, left);
    (stored & !0xff_0000) | (number << 16)
}

pub extern "system" fn SetThreadAffinityMask(thread: Handle, mask: usize) -> usize {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SetThreadAffinityMask(thread: Handle, mask: usize) -> usize;
    }
    let shift = pin_shift();
    let count = cpu_count();
    let shifted = rotate(mask, shift, true, count);
    // SAFETY: the title's thread handle and a mask of the same CPU count.
    let previous = unsafe { SetThreadAffinityMask(thread, shifted) };
    if shifted != mask {
        // SAFETY: this thread's last error, kept across the logging.
        let error = unsafe { last_error() };
        let first = SEEN
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values
            .insert(("SetThreadAffinityMask shifted", mask as u64));
        if first {
            tracing::info!(
                original = format_args!("{mask:#x}"),
                shifted = format_args!("{shifted:#x}"),
                "topology: affinity mask shifted"
            );
        }
        // SAFETY: as above.
        unsafe { set_last_error(error) };
    }
    note("SetThreadAffinityMask", mask as u64);
    // The title may restore the previous mask; hand it back in its own terms.
    rotate(previous, shift, false, count)
}

/// Shifted like the masks, so ideal and affinity stay on the same CPUs.
pub extern "system" fn SetThreadIdealProcessor(thread: Handle, processor: u32) -> u32 {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SetThreadIdealProcessor(thread: Handle, processor: u32) -> u32;
    }
    // SAFETY: the title's thread handle and a processor number below the count.
    let previous = unsafe { SetThreadIdealProcessor(thread, shift_processor(processor, true)) };
    note("SetThreadIdealProcessor", processor.into());
    // The failure value (u32::MAX) is past the count and passes unchanged.
    shift_processor(previous, false)
}

// Value: the `PROCESSOR_NUMBER` as stored (group low, number above it).
pub extern "system" fn SetThreadIdealProcessorEx(
    thread: Handle,
    ideal: *const u32,
    previous: *mut u32,
) -> i32 {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SetThreadIdealProcessorEx(thread: Handle, ideal: *const u32, previous: *mut u32)
        -> i32;
    }
    // SAFETY: not null, and the title passes its `PROCESSOR_NUMBER` there.
    let original = (!ideal.is_null()).then(|| unsafe { ideal.read_unaligned() });
    let shifted = original.map(|stored| shift_processor_number(stored, true));
    let shifted_ptr = shifted.as_ref().map_or(std::ptr::null(), |stored| stored as *const u32);
    // SAFETY: the title's handle and out pointer, and our copy of its number.
    let result = unsafe { SetThreadIdealProcessorEx(thread, shifted_ptr, previous) };
    if result != 0 && !previous.is_null() {
        // SAFETY: the host just wrote a `PROCESSOR_NUMBER` there.
        unsafe { previous.write_unaligned(shift_processor_number(previous.read_unaligned(), false)) };
    }
    note("SetThreadIdealProcessorEx", original.map_or(u64::MAX, u64::from));
    result
}
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
    fn rotation_wraps_in_the_cpu_count_and_keeps_the_count() {
        assert_eq!(rotate(0xf, 2, true, 16), 0x3c);
        assert_eq!(rotate(0xc000, 2, true, 16), 0x3);
        assert_eq!(rotate(0x3, 2, false, 16), 0xc000);
        assert_eq!(rotate(0xffff, 5, true, 16), 0xffff);
        assert_eq!(rotate(0x1_0001, 2, true, 16), 0x1_0001);
        assert_eq!(rotate(0, 2, true, 16), 0);
        assert_eq!(rotate(0x7, 0, true, 16), 0x7);
        assert_eq!(rotate(0xc00, 2, true, 12), 0x3);
        assert_eq!(rotate(0x1000, 2, true, 12), 0x1000);
        assert_eq!(rotate(1 << 63, 2, true, 64), 0x2);
        for mask in 1..=0xffffusize {
            let shifted = rotate(mask, 3, true, 16);
            assert_ne!(shifted, 0);
            assert_eq!(shifted.count_ones(), mask.count_ones());
            assert_eq!(rotate(shifted, 3, false, 16), mask);
        }
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
