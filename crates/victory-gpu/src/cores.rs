//! Which processors our own threads run on, where a processor has two kinds
//! of cores. `cpu_sets` and `keep_on_fast_cores` are fxceless/victory-fork's
//! (`framelog.rs`, commit 0b0074e0, GPL-3.0-or-later, as this).

/// One of the system's processor sets of group 0: its id, the physical core
/// it is a logical processor of, and its efficiency class (higher is faster).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Set {
    id: u32,
    core: u8,
    class: u8,
}

/// The system's processor sets of group 0.
fn cpu_sets() -> Vec<Set> {
    use windows_sys::Win32::System::SystemInformation::{GetSystemCpuSetInformation, SYSTEM_CPU_SET_INFORMATION};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let mut bytes = 0u32;
    // SAFETY: a size query, then the call into a buffer of that size.
    unsafe { GetSystemCpuSetInformation(std::ptr::null_mut(), 0, &mut bytes, GetCurrentProcess(), 0) };
    let mut buffer = vec![0u64; (bytes as usize).div_ceil(8).max(1)];
    // SAFETY: as above; the buffer holds `bytes` bytes, aligned for the records.
    let ok = unsafe { GetSystemCpuSetInformation(buffer.as_mut_ptr().cast(), bytes, &mut bytes, GetCurrentProcess(), 0) };
    let mut sets = Vec::new();
    let mut at = 0usize;
    while ok != 0 && at + size_of::<SYSTEM_CPU_SET_INFORMATION>() <= bytes as usize {
        // SAFETY: a record inside the buffer; each says its own size.
        let info = unsafe { &*(buffer.as_ptr().cast::<u8>().add(at).cast::<SYSTEM_CPU_SET_INFORMATION>()) };
        if info.Size == 0 {
            break;
        }
        // SAFETY: the record's type is CpuSet (the only one there is).
        let set = unsafe { &info.Anonymous.CpuSet };
        if set.Group == 0 {
            sets.push(Set {
                id: set.Id,
                core: set.CoreIndex,
                class: set.EfficiencyClass,
            });
        }
        at += info.Size as usize;
    }
    sets
}

/// The sets of the fastest class, if there are slower ones too.
fn fast_of(sets: &[Set]) -> Vec<u32> {
    let best = sets.iter().map(|c| c.class).max().unwrap_or(0);
    let fast: Vec<u32> = sets.iter().filter(|c| c.class == best).map(|c| c.id).collect();
    if fast.len() == sets.len() { Vec::new() } else { fast }
}

/// `VICTORY_SUBMIT_CORE` (off by default, for A/B): 1: the thread that
/// prepares and sends submissions (`victory-submit`, the one a frame waits
/// for) has one physical core of the fastest class to itself, both of its
/// logical processors, and our other threads that ask for the fast cores ask
/// for the rest of them; 2: as 1, and every thread of the process that asks
/// for nothing itself (the title's, the driver's) is kept off that core too
/// (the process's default sets; a thread the title pins by its affinity mask
/// stays where it is pinned).
///
/// Why: on a processor with two kinds of cores and few fast ones (an
/// i5-13600KF: 6 fast cores with two logical processors each, 8 slow ones)
/// the threads that ask for the fast cores (`VICTORY_FAST_CORES`, about ten
/// of them with the helpers) and the title's threads, which it pins to CPUs
/// 0-3, share six physical cores, so the submitting thread often runs beside
/// another busy thread on the same core, at a good part of its speed. A
/// processor of one kind (the 9800X3D the base was tuned on) asks for
/// nothing at all without this.
fn submit_core() -> u32 {
    static MODE: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *MODE.get_or_init(|| std::env::var("VICTORY_SUBMIT_CORE").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(0).min(2))
}

/// With [`submit_core`]: which sets the submitting thread is given, which
/// the other threads asking for fast cores are given, and which every
/// other thread of the process (mode 2).
#[derive(Debug, PartialEq, Eq)]
struct Layout {
    own: Vec<u32>,
    fast: Vec<u32>,
    rest: Vec<u32>,
}

/// The core of the fastest class with the highest index (the title pins its
/// threads to the lowest CPUs, `VICTORY_PIN_SPREAD` to the next ones, and
/// core 0 takes most of the system's interrupts), given to the submitting
/// thread alone. None with fewer than three physical cores of that class:
/// taking one would leave too little for the rest.
fn layout_of(sets: &[Set]) -> Option<Layout> {
    let best = sets.iter().map(|c| c.class).max()?;
    let mut cores: Vec<u8> = sets.iter().filter(|c| c.class == best).map(|c| c.core).collect();
    cores.sort_unstable();
    cores.dedup();
    if cores.len() < 3 {
        return None;
    }
    let mine = *cores.last()?;
    let own = sets.iter().filter(|c| c.class == best && c.core == mine).map(|c| c.id).collect();
    let fast = sets.iter().filter(|c| c.class == best && c.core != mine).map(|c| c.id).collect();
    let rest = sets.iter().filter(|c| !(c.class == best && c.core == mine)).map(|c| c.id).collect();
    Some(Layout { own, fast, rest })
}

/// `VICTORY_FAST_CORES`: 0: no thread asks; 1: the threads a frame waits for
/// one by one; 2 (the default): the threads they share their work with too;
/// 3: those that work beside the rest as well (lists translated, descriptors
/// made): more threads by then than the fast cores run at a time, while the
/// slow ones stood idle.
fn stage() -> u32 {
    static STAGE: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *STAGE.get_or_init(|| std::env::var("VICTORY_FAST_CORES").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(2))
}

/// Asks the system to run the calling thread, which a frame waits for, on
/// the fast cores only, where the processor has two kinds. That fork's submit
/// thread was found on a slow one in 1,529 looks of 24,021. (A selection of
/// processors is a preference the scheduler keeps unless nothing of it is
/// free.) The title's threads are not asked for.
pub(crate) fn keep_on_fast_cores(what: &str) {
    ask(what, 1);
}

/// [`keep_on_fast_cores`] for one of the threads a submission's preparing or
/// a landing shares its work with.
pub(crate) fn keep_helper_on_fast_cores(what: &str) {
    ask(what, 2);
}

/// [`keep_on_fast_cores`] for one of many threads whose work is ahead of what
/// a frame waits for.
pub(crate) fn keep_many_on_fast_cores(what: &str) {
    ask(what, 3);
}

/// The thread that prepares and sends submissions: with [`submit_core`], on
/// a physical core of its own (and with mode 2 the process's other threads
/// kept off it), else as [`keep_on_fast_cores`].
pub(crate) fn keep_submit_on_own_core(what: &str) {
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetCurrentThread, SetProcessDefaultCpuSets, SetThreadSelectedCpuSets};
    let Some(layout) = layout() else {
        ask(what, 1);
        return;
    };
    if submit_core() >= 2 {
        // SAFETY: our own process and a list of the system's own set ids.
        let ok = unsafe { SetProcessDefaultCpuSets(GetCurrentProcess(), layout.rest.as_ptr(), layout.rest.len() as u32) };
        if ok == 0 {
            tracing::warn!(error = %std::io::Error::last_os_error(), "the process's threads could not be kept off the submitting thread's core");
        }
    }
    // SAFETY: our own thread and a list of the system's own set ids.
    let ok = unsafe { SetThreadSelectedCpuSets(GetCurrentThread(), layout.own.as_ptr(), layout.own.len() as u32) };
    if ok == 0 {
        tracing::warn!(what, error = %std::io::Error::last_os_error(), "the thread could not be given a core of its own");
    } else {
        tracing::info!(
            what,
            sets = ?layout.own,
            others = layout.fast.len(),
            mode = submit_core(),
            "the thread has a fast core to itself (VICTORY_SUBMIT_CORE=0: not)"
        );
    }
}

/// [`layout_of`] the system's sets, once, if [`submit_core`] asks for it.
fn layout() -> Option<&'static Layout> {
    static LAYOUT: std::sync::OnceLock<Option<Layout>> = std::sync::OnceLock::new();
    if submit_core() == 0 {
        return None;
    }
    LAYOUT
        .get_or_init(|| {
            let layout = layout_of(&cpu_sets());
            if layout.is_none() {
                tracing::warn!("VICTORY_SUBMIT_CORE: fewer than three fast cores, nothing kept for the submitting thread");
            }
            layout
        })
        .as_ref()
}

fn ask(what: &str, from: u32) {
    use windows_sys::Win32::System::Threading::{GetCurrentThread, SetThreadSelectedCpuSets};
    if stage() < from {
        return;
    }
    static FAST: std::sync::OnceLock<(Vec<u32>, usize)> = std::sync::OnceLock::new();
    let (fast, of) = FAST.get_or_init(|| {
        let sets = cpu_sets();
        (fast_of(&sets), sets.len())
    });
    // With a core kept for the submitting thread: the other fast ones (on a
    // processor of one kind, all the others).
    let fast = layout().map_or(fast, |layout| &layout.fast);
    // All alike (or nothing known): nothing to ask for.
    if fast.is_empty() {
        return;
    }
    // SAFETY: our own thread and a list of the system's own set ids.
    let ok = unsafe { SetThreadSelectedCpuSets(GetCurrentThread(), fast.as_ptr(), fast.len() as u32) };
    if ok == 0 {
        tracing::warn!(what, error = %std::io::Error::last_os_error(), "the thread could not be kept on the fast cores");
    } else {
        tracing::info!(what, fast = fast.len(), of, "the thread is kept on the fast cores (VICTORY_FAST_CORES=0: not)");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(id: u32, core: u8, class: u8) -> Set {
        Set { id, core, class }
    }

    /// An i5-13600K(F): 6 fast cores of two logical processors each (sets
    /// 256-267, cores numbered by their first one), then 8 slow ones.
    fn raptor_lake_6p8e() -> Vec<Set> {
        let fast = (0..12u32).map(|i| set(256 + i, (i & !1) as u8, 1));
        let slow = (12..20u32).map(|i| set(256 + i, i as u8, 0));
        fast.chain(slow).collect()
    }

    #[test]
    fn the_fast_cores_are_those_of_the_best_class_if_there_is_a_worse_one() {
        assert_eq!(fast_of(&[set(256, 0, 1), set(257, 1, 1), set(258, 2, 0)]), [256, 257]);
        assert!(fast_of(&[set(256, 0, 0), set(257, 1, 0)]).is_empty());
        assert!(fast_of(&[]).is_empty());
        assert!(!cpu_sets().is_empty());
        std::thread::spawn(|| keep_on_fast_cores("test")).join().unwrap();
    }

    /// On a 6+8 core processor the submitting thread gets the last fast core,
    /// both of its logical processors; the other fast threads the five
    /// others; every other thread everything but that core.
    #[test]
    fn the_last_fast_core_is_kept_for_the_submitting_thread() {
        let layout = layout_of(&raptor_lake_6p8e()).unwrap();
        assert_eq!(layout.own, [266, 267]);
        assert_eq!(layout.fast, (256..266).collect::<Vec<_>>());
        assert_eq!(layout.rest, (256..266).chain(268..276).collect::<Vec<_>>());
    }

    /// On a processor of one kind (8 cores, 16 logical processors) the last
    /// core too, and the others are all the rest.
    #[test]
    fn a_processor_of_one_kind_keeps_its_last_core() {
        let sets: Vec<Set> = (0..16u32).map(|i| set(256 + i, (i / 2) as u8, 0)).collect();
        let layout = layout_of(&sets).unwrap();
        assert_eq!(layout.own, [270, 271]);
        assert_eq!(layout.fast, (256..270).collect::<Vec<_>>());
        assert_eq!(layout.fast, layout.rest);
    }

    /// Too few fast cores (2 fast and 8 slow, or nothing known): nothing kept.
    #[test]
    fn too_few_fast_cores_keep_nothing() {
        let mut sets: Vec<Set> = (0..4u32).map(|i| set(256 + i, (i & !1) as u8, 1)).collect();
        sets.extend((4..12u32).map(|i| set(256 + i, i as u8, 0)));
        assert_eq!(layout_of(&sets), None);
        assert_eq!(layout_of(&[]), None);
    }

    /// Off by default: the submitting thread asks as before.
    #[test]
    fn off_unless_asked() {
        if std::env::var_os("VICTORY_SUBMIT_CORE").is_none() {
            assert_eq!(submit_core(), 0);
            assert!(layout().is_none());
        }
        std::thread::spawn(|| keep_submit_on_own_core("test")).join().unwrap();
    }
}
