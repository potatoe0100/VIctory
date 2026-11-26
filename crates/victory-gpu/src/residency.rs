//! Host textures past the card's memory budget are evicted and kept: their
//! next use makes them resident again instead of making them anew.
//!
//! The console's memory is one pool the title manages itself. On the host
//! the video memory manager gives the process a budget of the card's
//! memory and pages what is used past it. The eviction pass
//! (`crate::resource::evict_unused`, once a presented frame) keeps the host
//! textures fed from title memory under that budget: it gives up (destroys)
//! those the title has not used for a long time and, past the budget, the
//! least recently used ones, which are made again from title memory at
//! their next use (detiled and uploaded). Past the budget a committed one
//! is evicted instead (`ID3D12Device::Evict`): the manager may page it out,
//! its contents and GPU address stay, and its next use makes it resident
//! again (`MakeResident`: a copy back if it was paged out, nothing
//! otherwise). The budget's arithmetic takes it that evicted memory leaves
//! the card's use at once: the first eviction of a run checks that, and
//! where it does not hold, textures are given up as before ([`evict`]).
//!
//! The device changes the residency of heaps and committed resources, not
//! of placed ones: host textures in heaps of textures (`crate::placement`)
//! are given up as before.
//! planned: evict a heap of textures whose textures all went unused.
//!
//! The GPU must reach no evicted texture:
//! * lists name the textures they use (`HostList::transition`): before they
//!   run, those of them that are evicted are made resident
//!   ([`before_running`]);
//! * shaders read views through any slot of the heap: the pass leaves the
//!   slots viewing a texture null before it evicts it, a use makes it
//!   resident before a view of it is made (`Resource::host`), and a slot
//!   made while a pass picked or evicted its texture is checked once it
//!   holds it (`HostHeap::check_resident`).
//!
//! The device counts residency: a resource is resident when made (count 1),
//! `Evict` takes one and `MakeResident` gives one. [`Residency`] keeps each
//! texture's count at 1 or 0: every change is made with the device call it
//! needs, under one lock.

use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};

/// A host texture fed from title memory that the title has not used for
/// this long (and this many presented frames) is given up
/// (`crate::resource::evict`). Two minutes: of the textures given up after
/// 20 s, nine in ten were read again within a minute (run evict4).
pub(crate) const IDLE_MS: u64 = 120_000;
/// Frames as well as time: in slow runs (seconds a frame) 20 s alone cycled
/// the textures of a cutscene's alternating shots (run evict2), and 30
/// frames gave up what the menus and loading made (at 30 fps) before the
/// world read it (run evict3). At 30 fps the idle time decides; slower, the
/// card's budget does.
pub(crate) const IDLE_FRAMES: u64 = 300;
/// Past this share of the card's memory budget, host textures fed from
/// title memory are let go of least recently used first, until the card's
/// use is back under [`BUDGET_LOW`]. The gap keeps the pass from running
/// again at every texture made at the budget.
const BUDGET_HIGH: f64 = 0.90;
const BUDGET_LOW: f64 = 0.85;

/// What the eviction pass knows of a host texture it may let go of.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Use {
    /// When the title last used it: presented frame and milliseconds.
    pub(crate) frame: u64,
    pub(crate) ms: u64,
    /// Bytes it takes on the card.
    pub(crate) bytes: u64,
    pub(crate) kind: Kind,
}

/// How a host texture can leave the card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Only by being given up: it is placed in a heap of textures, or the
    /// host cannot evict.
    OnlyGivenUp,
    /// It may be evicted (committed).
    Evictable,
    /// It is evicted already.
    Evicted,
}

/// What an eviction pass does: the textures (indices) it gives up, and
/// those it evicts.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Plan {
    pub(crate) give_up: Vec<usize>,
    pub(crate) evict: Vec<usize>,
}

/// The eviction pass's choice among textures `uses` at (`frame`, `now`
/// ms), `over` bytes past the budget: every one idle for [`IDLE_FRAMES`]
/// and [`IDLE_MS`] is given up, evicted or not; then the least recently
/// used of the others on the card until `over` bytes are off it, evictable
/// ones evicted and the rest given up. An evicted texture makes no room.
pub(crate) fn plan(uses: &[Use], frame: u64, now: u64, over: u64) -> Plan {
    let idle = |u: &Use| {
        frame.saturating_sub(u.frame) >= IDLE_FRAMES && now.saturating_sub(u.ms) >= IDLE_MS
    };
    let (give_up, mut rest): (Vec<usize>, Vec<usize>) =
        (0..uses.len()).partition(|&i| idle(&uses[i]));
    let on_card = |i: &usize| uses[*i].kind != Kind::Evicted;
    let freed: u64 = give_up
        .iter()
        .filter(|i| on_card(i))
        .map(|&i| uses[i].bytes)
        .sum();
    let mut over = over.saturating_sub(freed);
    rest.retain(on_card);
    rest.sort_by_key(|&i| uses[i].ms);
    let mut plan = Plan {
        give_up,
        evict: Vec::new(),
    };
    for i in rest {
        if over == 0 {
            break;
        }
        over = over.saturating_sub(uses[i].bytes);
        match uses[i].kind {
            Kind::Evictable => plan.evict.push(i),
            _ => plan.give_up.push(i),
        }
    }
    plan
}

/// Bytes to let go of with `used` bytes of the card in use and a budget of
/// `budget`: none up to [`BUDGET_HIGH`] of it, past that all down to
/// [`BUDGET_LOW`].
pub(crate) fn over(used: u64, budget: u64) -> u64 {
    let (used, budget) = (used as f64, budget as f64);
    if used > BUDGET_HIGH * budget {
        (used - BUDGET_LOW * budget) as u64
    } else {
        0
    }
}

/// The budget `VICTORY_VRAM_BUDGET_MB` (value `value`) sets, in bytes: a
/// count of MB of 2^20 bytes (8192 for an 8 GB card). None without one,
/// or for a value that is no positive count.
#[cfg(any(test, feature = "diagnostics"))]
pub(crate) fn budget_limit(value: Option<&str>) -> Option<u64> {
    let mb = value?.trim().parse::<u64>().ok().filter(|&mb| mb > 0)?;
    Some(mb.saturating_mul(1 << 20))
}

/// Whether evicting frees budget on an adapter with 2^`bits` bytes of GPU
/// addresses per process and a budget of `budget` bytes. Evicted resources
/// keep their addresses, so where the address space is about the budget's
/// size it is what limits the process, and `Evict` frees nothing
/// (`MakeResident`'s documentation). Four times the budget is taken as
/// enough.
pub(crate) fn address_space_allows(bits: u32, budget: u64) -> bool {
    bits > 0 && (bits >= 64 || 1u64 << bits >= budget.saturating_mul(4))
}

/// Host textures picked or evicted ([`Residency`]), and their bytes.
#[derive(Debug, Default)]
pub(crate) struct Out {
    count: AtomicU64,
    bytes: AtomicU64,
}

impl Out {
    pub(crate) const fn new() -> Self {
        Out {
            count: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
        }
    }

    /// Every host texture is resident and picked by no pass. A pick made
    /// before a lock the caller took since shows here.
    pub(crate) fn none(&self) -> bool {
        self.count.load(Ordering::Acquire) == 0
    }

    /// Host textures picked or evicted now, and their bytes.
    pub(crate) fn now(&self) -> (u64, u64) {
        (
            self.count.load(Ordering::Relaxed),
            self.bytes.load(Ordering::Relaxed),
        )
    }

    fn add(&self, bytes: u64) {
        self.count.fetch_add(1, Ordering::AcqRel);
        self.bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    fn take(&self, bytes: u64) {
        self.count.fetch_sub(1, Ordering::AcqRel);
        self.bytes.fetch_sub(bytes, Ordering::Relaxed);
    }
}

/// A host texture's residency as eviction passes change it: resident (the
/// device's count 1, as made), picked by a pass that has not evicted it yet
/// (still 1), or evicted (0). Changed under the residency lock together
/// with the device call the change needs, so the count stays 1 or 0, and
/// counted in an [`Out`] while not resident; read without the lock to skip
/// it when there is nothing to do.
#[derive(Debug, Default)]
pub(crate) struct Residency(AtomicU8);

const RESIDENT: u8 = 0;
const PICKED: u8 = 1;
const EVICTED: u8 = 2;

impl Residency {
    /// Resident, and picked by no pass: a use has nothing to do. A pick
    /// made before a lock the caller took since shows here.
    pub(crate) fn is_resident(&self) -> bool {
        self.0.load(Ordering::Acquire) == RESIDENT
    }

    /// A pass picks it for eviction (of `bytes`): whether it was resident,
    /// and then it is picked.
    fn pick(&self, out: &Out, bytes: u64) -> bool {
        let picked = self
            .0
            .compare_exchange(RESIDENT, PICKED, Ordering::AcqRel, Ordering::Acquire);
        if picked.is_ok() {
            out.add(bytes);
        }
        picked.is_ok()
    }

    /// The pass evicts it, the heap slots viewing it left null: whether it
    /// is still picked (no use undid that since), and then `Evict` is the
    /// call to make.
    fn evict(&self) -> bool {
        self.0
            .compare_exchange(PICKED, EVICTED, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    /// The device refused to evict it: resident as before.
    fn evict_refused(&self, out: &Out, bytes: u64) {
        if self.0.swap(RESIDENT, Ordering::AcqRel) != RESIDENT {
            out.take(bytes);
        }
    }

    /// A use: whether it is evicted, and `MakeResident` the call to make
    /// first ([`Self::made_resident`] once it is). A pick is undone.
    fn used(&self, out: &Out, bytes: u64) -> bool {
        match self.0.load(Ordering::Acquire) {
            PICKED => {
                self.0.store(RESIDENT, Ordering::Release);
                out.take(bytes);
                false
            }
            EVICTED => true,
            _ => false,
        }
    }

    /// `MakeResident` made it resident again.
    fn made_resident(&self, out: &Out, bytes: u64) {
        if self.0.swap(RESIDENT, Ordering::AcqRel) != RESIDENT {
            out.take(bytes);
        }
    }

    /// The texture goes: no longer counted (an evicted one is released as
    /// it is).
    pub(crate) fn dropped(&self, out: &Out, bytes: u64) {
        if self.0.load(Ordering::Acquire) != RESIDENT {
            out.take(bytes);
        }
    }
}

#[cfg(windows)]
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

#[cfg(windows)]
use crate::resource::HostTexture;

/// Host textures picked or evicted now ([`Residency`]).
#[cfg(windows)]
pub(crate) static OUT: Out = Out::new();

/// Since the last report: host textures evicted and their bytes, then those
/// made resident again and their bytes.
#[cfg(windows)]
static MOVES: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];

/// Taken to change a texture's [`Residency`] with the device call the
/// change needs.
#[cfg(windows)]
static LOCK: Mutex<()> = Mutex::new(());

#[cfg(windows)]
fn lock() -> MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Whether every host texture is resident and picked by no pass: nothing
/// to check. A pick made before a lock the caller took since shows here.
#[cfg(windows)]
pub(crate) fn all_resident() -> bool {
    OUT.none()
}

/// The first eviction did not lower the card's use ([`evict`]): textures
/// past the budget are given up from then on.
#[cfg(windows)]
static INEFFECTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether host textures past the budget may be evicted (otherwise they are
/// given up): the adapter's address space is large enough
/// ([`address_space_allows`]), the first eviction lowered the card's use,
/// and in diagnostics builds `VICTORY_GIVE_UP_TEXTURES` is not on.
#[cfg(windows)]
pub(crate) fn available() -> bool {
    use windows::Win32::Graphics::Direct3D12::{
        D3D12_FEATURE_DATA_GPU_VIRTUAL_ADDRESS_SUPPORT, D3D12_FEATURE_GPU_VIRTUAL_ADDRESS_SUPPORT,
    };
    static ON: OnceLock<bool> = OnceLock::new();
    if INEFFECTIVE.load(Ordering::Relaxed) {
        return false;
    }
    *ON.get_or_init(|| {
        if cfg!(feature = "diagnostics") && crate::switch::on("VICTORY_GIVE_UP_TEXTURES") {
            tracing::info!(
                "VICTORY_GIVE_UP_TEXTURES: host textures past the budget are given up, not evicted"
            );
            return false;
        }
        let Some(host) = crate::host::host() else {
            return false;
        };
        let mut va = D3D12_FEATURE_DATA_GPU_VIRTUAL_ADDRESS_SUPPORT::default();
        // SAFETY: a plain query into a structure of the size passed.
        let bits = unsafe {
            host.device.CheckFeatureSupport(
                D3D12_FEATURE_GPU_VIRTUAL_ADDRESS_SUPPORT,
                std::ptr::from_mut(&mut va).cast(),
                std::mem::size_of_val(&va) as u32,
            )
        }
        .map_or(0, |()| va.MaxGPUVirtualAddressBitsPerProcess);
        let budget =
            crate::host::video_memory().map_or(0, |[(_, budget), _]| (budget * 1e6) as u64);
        let on = address_space_allows(bits, budget);
        if !on {
            tracing::info!(
                bits,
                budget_mb = budget / 1_000_000,
                "host textures past the budget are given up, not evicted: the GPU's address space \
                 per process is not much larger than the budget, which evicting would not free"
            );
        }
        on
    })
}

/// Bytes of host textures the eviction pass lets go of now ([`over`]): the
/// card's use counts without the free space in the heaps of textures, which
/// takes the card's memory but is not in use (new textures go there first).
/// With `VICTORY_VRAM_BUDGET_MB` (diagnostics builds) the budget is the
/// smaller of the card's and that.
#[cfg(windows)]
pub(crate) fn over_budget() -> u64 {
    let Some([(used, budget), _]) = crate::host::video_memory() else {
        return 0;
    };
    let used = ((used * 1e6) as u64).saturating_sub(crate::placement::free_bytes());
    over(used, limited((budget * 1e6) as u64))
}

/// The budget the pass keeps to: the card's `budget`, or a smaller one that
/// `VICTORY_VRAM_BUDGET_MB` sets (diagnostics builds: a smaller card on a
/// larger one).
#[cfg(windows)]
fn limited(budget: u64) -> u64 {
    #[cfg(feature = "diagnostics")]
    {
        static LIMIT: OnceLock<Option<u64>> = OnceLock::new();
        let limit = *LIMIT.get_or_init(|| {
            let value = std::env::var("VICTORY_VRAM_BUDGET_MB").ok();
            let limit = budget_limit(value.as_deref());
            match (&value, limit) {
                (Some(_), Some(limit)) => tracing::info!(
                    mb = limit >> 20,
                    "VICTORY_VRAM_BUDGET_MB: host textures kept under at most this budget"
                ),
                (Some(value), None) => tracing::warn!(
                    value = %value,
                    "VICTORY_VRAM_BUDGET_MB expects a positive count of MB; the card's budget is kept to"
                ),
                (None, _) => {}
            }
            limit
        });
        if let Some(limit) = limit {
            return budget.min(limit);
        }
    }
    budget
}

/// The first step of evicting host textures `textures`: those picked (the
/// resident ones). The pass then leaves the heap slots viewing them null
/// (`crate::descriptor::evict`) and evicts them ([`evict`]); a slot made
/// meanwhile makes its texture resident once it holds it.
#[cfg(windows)]
pub(crate) fn pick(textures: Vec<Arc<HostTexture>>) -> Vec<Arc<HostTexture>> {
    let _one = lock();
    textures
        .into_iter()
        .filter(|t| t.residency.pick(&OUT, t.bytes()))
        .collect()
}

/// Evicts in one call the textures [`pick`] picked that no use took back
/// since, once no heap slot views them. The GPU runs nothing meanwhile:
/// passes run at the start of a submission, after the last one ran. How
/// many it evicted, and their bytes.
///
/// The first eviction of a run reads the card's use just before and after
/// the call and logs both. If it did not drop by a quarter of what was
/// evicted, the budget's arithmetic does not hold on this host (each pass
/// would evict more): textures are given up from then on, as they were
/// before eviction. Not in tests, whose textures come and go in parallel.
#[cfg(windows)]
pub(crate) fn evict(picked: &[Arc<HostTexture>]) -> (u64, u64) {
    use windows::Win32::Graphics::Direct3D12::ID3D12Pageable;
    if picked.is_empty() {
        return (0, 0);
    }
    static FIRST: std::sync::Once = std::sync::Once::new();
    let measuring = !cfg!(test) && !FIRST.is_completed();
    let one = lock();
    let evicted: Vec<&Arc<HostTexture>> = picked.iter().filter(|t| t.residency.evict()).collect();
    if evicted.is_empty() {
        return (0, 0);
    }
    let before = if measuring {
        crate::host::video_memory()
    } else {
        None
    };
    let made = crate::host::host().map(|host| {
        let pageables: Vec<Option<ID3D12Pageable>> = evicted
            .iter()
            .map(|t| Some(ID3D12Pageable::clone(&t.texture)))
            .collect();
        // SAFETY: live committed textures, which no GPU work running or
        // submitted names (above).
        unsafe { host.device.Evict(&pageables) }
    });
    let after = if measuring {
        crate::host::video_memory()
    } else {
        None
    };
    if let Some(Err(e)) = &made {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            tracing::warn!(error = %e, "the host refused to evict textures past the budget: they stay");
        });
    }
    if !matches!(made, Some(Ok(()))) {
        for t in &evicted {
            t.residency.evict_refused(&OUT, t.bytes());
        }
        return (0, 0);
    }
    drop(one);
    let (n, bytes) = (
        evicted.len() as u64,
        evicted.iter().map(|t| t.bytes()).sum::<u64>(),
    );
    MOVES[0].fetch_add(n, Ordering::Relaxed);
    MOVES[1].fetch_add(bytes, Ordering::Relaxed);
    FIRST.call_once(|| {
        let (Some([(was, budget), _]), Some([(is, _), _])) = (before, after) else {
            return;
        };
        tracing::info!(
            "first host textures evicted past the budget: {n}, {:.0} MB; the card's use was \
             {was:.0} MB of a {budget:.0} MB budget, is {is:.0} MB",
            bytes as f64 / 1e6
        );
        let lowered = ((was - is) * 1e6).max(0.0) as u64;
        if lowered < bytes / 4 {
            INEFFECTIVE.store(true, Ordering::Relaxed);
            tracing::warn!(
                "evicting did not lower the card's use: host textures past the budget are given up \
                 from now on"
            );
        }
    });
    (n, bytes)
}

/// A use of host texture `t`: made resident first if a pass evicted it (a
/// pick is undone). Whether it is resident: if the device refuses (no
/// room), a submission naming it makes room first ([`before_running`]),
/// and a heap slot viewing it is left null.
#[cfg(windows)]
pub(crate) fn make_resident(t: &HostTexture) -> bool {
    if t.residency.is_resident() {
        return true;
    }
    let Some(host) = crate::host::host() else {
        return false;
    };
    let _one = lock();
    if !t.residency.used(&OUT, t.bytes()) {
        return true;
    }
    let made = restore(&host.device, &[t]);
    if let Err(e) = &made {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            tracing::warn!(error = %e, "an evicted host texture could not be made resident at its use");
        });
    }
    made.is_ok()
}

/// Makes evicted textures `textures` (each once) resident in one call; under
/// the lock.
#[cfg(windows)]
fn restore(
    device: &windows::Win32::Graphics::Direct3D12::ID3D12Device,
    textures: &[&HostTexture],
) -> windows::core::Result<()> {
    use windows::Win32::Graphics::Direct3D12::ID3D12Pageable;
    let pageables: Vec<Option<ID3D12Pageable>> = textures
        .iter()
        .map(|t| Some(ID3D12Pageable::clone(&t.texture)))
        .collect();
    // SAFETY: live committed textures that we evicted.
    unsafe { device.MakeResident(&pageables) }?;
    let mut bytes = 0;
    for t in textures {
        t.residency.made_resident(&OUT, t.bytes());
        bytes += t.bytes();
    }
    MOVES[2].fetch_add(textures.len() as u64, Ordering::Relaxed);
    MOVES[3].fetch_add(bytes, Ordering::Relaxed);
    Ok(())
}

/// Before lists `lists` run: the host textures they use that are evicted
/// are made resident, in one call. If the device refuses (no room), others
/// are let go of first, least recently used, none that these lists use;
/// refused again, the run ends: the lists would fault the GPU.
#[cfg(windows)]
pub(crate) fn before_running(lists: &[&crate::host::HostList]) {
    if all_resident() {
        return;
    }
    before_using(lists.iter().flat_map(|l| l.textures()).collect());
}

/// Before GPU work that names host textures `used` outside a submission's
/// lists (the present's own copy list): the same as [`before_running`].
#[cfg(windows)]
pub(crate) fn before_using(used: Vec<Arc<HostTexture>>) {
    if all_resident() {
        return;
    }
    let Some(host) = crate::host::host() else {
        return;
    };
    let mut named = std::collections::HashSet::new();
    let used: Vec<Arc<HostTexture>> = used
        .into_iter()
        .filter(|t| named.insert(Arc::as_ptr(t) as usize))
        .collect();
    // The evicted ones made resident: the error, how many, their bytes.
    let attempt = || -> Result<(), (windows::core::Error, usize, u64)> {
        let _one = lock();
        let evicted: Vec<&HostTexture> = used
            .iter()
            .filter(|t| t.residency.used(&OUT, t.bytes()))
            .map(|t| &**t)
            .collect();
        if evicted.is_empty() {
            return Ok(());
        }
        let bytes = evicted.iter().map(|t| t.bytes()).sum();
        restore(&host.device, &evicted).map_err(|e| (e, evicted.len(), bytes))
    };
    let Err((_, _, bytes)) = attempt() else {
        return;
    };
    crate::resource::make_room(bytes, &named);
    if let Err((e, n, bytes)) = attempt() {
        crate::host::fail(format_args!(
            "{n} host textures evicted past the card's budget ({:.0} MB) could not be made \
             resident again for a submission ({e})",
            bytes as f64 / 1e6
        ));
    }
}

/// The frame-rate line's companion: host textures evicted past the budget
/// and made resident again since the last line, per frame (`frames`), and
/// those evicted now. Nothing if none were.
#[cfg(windows)]
pub(crate) fn log(frames: f64) {
    let [n, bytes, back, back_bytes] = MOVES.each_ref().map(|m| m.swap(0, Ordering::Relaxed));
    let (out, out_bytes) = OUT.now();
    if n + back + out == 0 {
        return;
    }
    tracing::info!(
        "host textures evicted past the budget: {:.1}/frame ({:.1} MB/frame), made resident again \
         {:.1}/frame ({:.1} MB/frame); evicted now {out} ({:.0} MB)",
        n as f64 / frames,
        bytes as f64 / frames / 1e6,
        back as f64 / frames,
        back_bytes as f64 / frames / 1e6,
        out_bytes as f64 / 1e6
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(frame: u64, ms: u64, bytes: u64, kind: Kind) -> Use {
        Use {
            frame,
            ms,
            bytes,
            kind,
        }
    }

    /// Textures idle for the idle time and frames are given up; past the
    /// budget the least recently used others follow until the excess is
    /// gone (textures that can only be given up: the pass as it was before
    /// it evicted).
    #[test]
    fn idle_textures_go_and_past_the_budget_the_least_recently_used() {
        let (frame, now) = (1_000, 1_000_000);
        let given_up = |frame, ms, bytes| at(frame, ms, bytes, Kind::OnlyGivenUp);
        let recent = given_up(frame, now - 1_000, 10);
        let idle = given_up(frame - IDLE_FRAMES, now - IDLE_MS, 10);
        let older = given_up(frame, now - 5_000, 30);
        let uses = [recent, idle, older];
        let gone = |over| plan(&uses, frame, now, over);
        let only = |give_up: Vec<usize>| Plan {
            give_up,
            evict: vec![],
        };
        assert_eq!(gone(0), only(vec![1]), "idle only");
        assert_eq!(gone(10), only(vec![1]), "the idle one covers the excess");
        assert_eq!(gone(11), only(vec![1, 2]), "then the least recently used");
        assert_eq!(gone(1 << 20), only(vec![1, 2, 0]), "all of them if need be");
        let early = given_up(frame - 1, now - IDLE_MS, 10);
        assert_eq!(
            plan(&[early], frame, now, 0),
            only(vec![]),
            "too few frames ago"
        );
    }

    #[test]
    fn idle_textures_are_given_up_evicted_or_not() {
        let uses = [
            at(0, 0, 10, Kind::Evictable),
            at(0, 0, 20, Kind::Evicted),
            at(0, 0, 30, Kind::OnlyGivenUp),
        ];
        let p = plan(&uses, IDLE_FRAMES, IDLE_MS, 0);
        assert_eq!(
            p,
            Plan {
                give_up: vec![0, 1, 2],
                evict: vec![]
            }
        );
    }

    #[test]
    fn past_the_budget_the_least_recently_used_go_until_it_is_covered() {
        let (frame, now) = (1000, 1_000_000);
        let uses = [
            at(frame, now - 10, 100, Kind::Evictable),
            at(frame, now - 30, 100, Kind::OnlyGivenUp),
            at(frame, now - 20, 100, Kind::Evictable),
            at(frame, now, 100, Kind::Evictable),
        ];
        // 150 bytes: the two least recently used, the oldest given up (in a
        // heap of textures), the next evicted.
        let p = plan(&uses, frame, now, 150);
        assert_eq!(
            p,
            Plan {
                give_up: vec![1],
                evict: vec![2]
            }
        );
        // Past all of them: every one, oldest first.
        let p = plan(&uses, frame, now, u64::MAX);
        assert_eq!(
            p,
            Plan {
                give_up: vec![1],
                evict: vec![2, 0, 3]
            }
        );
    }

    #[test]
    fn evicted_textures_make_no_room_on_the_card() {
        let (frame, now) = (1000, 1_000_000);
        let uses = [
            at(frame, now - 50, 100, Kind::Evicted),
            at(frame, now - 10, 100, Kind::Evictable),
            // Idle and evicted: given up, but it frees nothing of the card.
            at(0, 0, 1000, Kind::Evicted),
        ];
        let p = plan(&uses, frame, now, 50);
        assert_eq!(
            p,
            Plan {
                give_up: vec![2],
                evict: vec![1]
            }
        );
    }

    #[test]
    fn idle_textures_on_the_card_count_toward_the_budget() {
        let (frame, now) = (1000, 1_000_000);
        let uses = [
            at(0, 0, 100, Kind::OnlyGivenUp),
            at(frame, now, 100, Kind::Evictable),
        ];
        assert_eq!(
            plan(&uses, frame, now, 100),
            Plan {
                give_up: vec![0],
                evict: vec![]
            }
        );
        assert_eq!(
            plan(&uses, frame, now, 101),
            Plan {
                give_up: vec![0],
                evict: vec![1]
            }
        );
    }

    #[test]
    fn the_pass_lets_go_past_the_high_mark_down_to_the_low_one() {
        let budget = 10_000;
        assert_eq!(over(8_000, budget), 0);
        assert_eq!(over(9_000, budget), 0, "at the high mark: nothing yet");
        assert_eq!(over(9_001, budget), 501);
        assert_eq!(over(12_000, budget), 3_500);
        assert_eq!(over(0, 0), 0);
    }

    #[test]
    fn a_budget_limit_is_a_positive_count_of_mb() {
        assert_eq!(budget_limit(Some("8192")), Some(8 << 30));
        assert_eq!(budget_limit(Some(" 512 ")), Some(512 << 20));
        assert_eq!(
            budget_limit(Some(&u64::MAX.to_string())),
            Some(u64::MAX),
            "saturates"
        );
        for refused in ["0", "", "-1", "8 GB", "1.5"] {
            assert_eq!(budget_limit(Some(refused)), None, "{refused:?}");
        }
        assert_eq!(budget_limit(None), None);
    }

    #[test]
    fn evicting_needs_an_address_space_well_past_the_budget() {
        let gb = 1u64 << 30;
        assert!(address_space_allows(40, 12 * gb), "1 TB of addresses");
        assert!(
            address_space_allows(36, 16 * gb),
            "64 GB: four times the budget"
        );
        assert!(!address_space_allows(35, 16 * gb));
        assert!(!address_space_allows(32, 12 * gb), "4 GB of addresses");
        assert!(!address_space_allows(0, gb), "unknown");
        assert!(address_space_allows(64, u64::MAX));
    }

    /// One step of the code that changes residencies, under its lock: an
    /// eviction pass picking the texture, the pass evicting what it picked
    /// (the device may refuse), or a use (making it resident may be
    /// refused).
    #[derive(Debug, Clone, Copy)]
    enum Op {
        Pick,
        Evict { refused: bool },
        Use { refused: bool },
    }

    /// Applies `op` as the device code does, with the device's count for
    /// the texture in `count`.
    fn apply(r: &Residency, out: &Out, count: &mut i32, op: Op) {
        const BYTES: u64 = 64;
        match op {
            Op::Pick => {
                r.pick(out, BYTES);
            }
            Op::Evict { refused } => {
                if r.evict() {
                    if refused {
                        r.evict_refused(out, BYTES);
                    } else {
                        *count -= 1;
                    }
                }
            }
            Op::Use { refused } => {
                if r.used(out, BYTES) && !refused {
                    *count += 1;
                    r.made_resident(out, BYTES);
                }
            }
        }
    }

    /// Every order of `sequences` (each in its own order), appended to
    /// `order`, passed to `f`.
    fn orders(sequences: &mut [Vec<Op>], order: &mut Vec<Op>, f: &mut impl FnMut(&[Op])) {
        if sequences.iter().all(Vec::is_empty) {
            f(order);
            return;
        }
        for i in 0..sequences.len() {
            if sequences[i].is_empty() {
                continue;
            }
            let op = sequences[i].remove(0);
            order.push(op);
            orders(sequences, order, f);
            order.pop();
            sequences[i].insert(0, op);
        }
    }

    /// Two passes, one after the other (passes run one at a time), and two
    /// uses on other threads in every order between their steps, with every
    /// refusal: the device's count stays 1 for a resident or picked texture
    /// and 0 for an evicted one, never twice either, and the texture is
    /// counted out while it is not resident. Each step does its part: a pick
    /// picks a resident texture, an eviction the device takes evicts a
    /// picked one, and a use the device does not refuse leaves it resident.
    #[test]
    fn residency_counts_stay_one_or_zero_in_every_order() {
        let mut runs = 0;
        for refusals in 0..16u32 {
            let refused = |bit: u32| refusals & 1 << bit != 0;
            let passes = vec![
                Op::Pick,
                Op::Evict {
                    refused: refused(0),
                },
                Op::Pick,
                Op::Evict {
                    refused: refused(1),
                },
            ];
            let mut sequences = [
                passes,
                vec![Op::Use {
                    refused: refused(2),
                }],
                vec![Op::Use {
                    refused: refused(3),
                }],
            ];
            orders(&mut sequences, &mut Vec::new(), &mut |order| {
                let (r, out, mut count) = (Residency::default(), Out::new(), 1);
                for &op in order {
                    let was = r.0.load(Ordering::Relaxed);
                    apply(&r, &out, &mut count, op);
                    let state = r.0.load(Ordering::Relaxed);
                    match op {
                        Op::Pick if was == RESIDENT => assert_eq!(state, PICKED, "{order:?}"),
                        Op::Evict { refused: false } if was == PICKED => {
                            assert_eq!(state, EVICTED, "{order:?}");
                        }
                        Op::Use { refused: false } => assert_eq!(state, RESIDENT, "{order:?}"),
                        _ => {}
                    }
                    let expected = if state == EVICTED { 0 } else { 1 };
                    assert_eq!(count, expected, "after {op:?} in {order:?}");
                    let counted = out.now();
                    assert_eq!(
                        counted.0,
                        u64::from(state != RESIDENT),
                        "counted out, {order:?}"
                    );
                    assert_eq!(counted.1, if state != RESIDENT { 64 } else { 0 });
                }
                r.dropped(&out, 64);
                assert_eq!(out.now(), (0, 0), "nothing counted once it is gone");
                runs += 1;
            });
        }
        // 6!/4! orders of the steps, for each of the 16 refusal sets.
        assert_eq!(runs, 30 * 16);
    }

    #[test]
    fn a_use_takes_a_pick_back_before_the_pass_evicts() {
        let (r, out) = (Residency::default(), Out::new());
        assert!(r.pick(&out, 1) && !r.is_resident());
        assert!(!r.pick(&out, 1), "picked once");
        assert!(!r.used(&out, 1), "nothing to make resident");
        assert!(r.is_resident() && out.none());
        assert!(!r.evict(), "the pass leaves it");
    }
}

/// Eviction on the host device (a GPU, or the software adapter).
#[cfg(all(test, windows))]
mod device_tests {
    use std::collections::HashSet;
    use std::sync::Arc;

    /// A 4096x2048 RGBA8 texture, linear, in title memory of its own:
    /// 32 MiB, more than a heap of textures takes, so committed.
    fn committed_texture(memory: &[u8]) -> crate::object::This {
        let words: [u32; 14] = [
            3,
            0,
            0,
            0,
            4096,
            0,
            2048,
            1 | 1 << 16,
            28,
            1,
            0,
            0x100,
            0,
            0,
        ];
        crate::resource::texture_at(memory.as_ptr() as u64, &words).expect("its resource")
    }

    /// Lets go of `texture` as an eviction pass far past the budget does.
    fn evict_now(texture: crate::object::This) {
        let owned = crate::resource::owned(texture).expect("may be let go of");
        crate::resource::evict_among(&[owned], &HashSet::new(), 0, 0, u64::MAX);
    }

    #[test]
    fn a_committed_texture_past_the_budget_is_evicted_kept_and_made_resident_by_a_use() {
        if crate::host::host().is_none() || !super::available() {
            return;
        }
        let memory = vec![0u8; 32 << 20];
        let texture = committed_texture(&memory);
        let copy = crate::resource::host_copy(texture).expect("a host texture");
        assert!(copy.committed(), "too large for a heap of textures");
        evict_now(texture);
        assert!(!copy.residency.is_resident(), "evicted");
        let kept = crate::resource::made_copy(texture).expect("kept, not given up");
        assert!(Arc::ptr_eq(&kept, &copy), "the same host texture");
        let used = crate::resource::host_copy(texture).expect("its host texture");
        assert!(Arc::ptr_eq(&used, &copy), "no new host texture");
        assert!(copy.residency.is_resident(), "a use made it resident");
    }

    #[test]
    fn work_outside_a_submission_makes_its_evicted_textures_resident() {
        if crate::host::host().is_none() || !super::available() {
            return;
        }
        let memory = vec![0u8; 32 << 20];
        let texture = committed_texture(&memory);
        let copy = crate::resource::host_copy(texture).expect("a host texture");
        evict_now(texture);
        assert!(!copy.residency.is_resident(), "evicted");
        // As the present does for its planes before its own copy list runs.
        super::before_using(vec![crate::resource::made_copy(texture).expect("kept")]);
        assert!(copy.residency.is_resident(), "made resident before use");
    }

    #[test]
    fn a_list_naming_a_texture_evicted_after_it_was_recorded_runs_with_it_resident() {
        let Some(host) = crate::host::host() else {
            return;
        };
        if !super::available() {
            return;
        }
        let memory = vec![0u8; 32 << 20];
        let texture = committed_texture(&memory);
        let copy = crate::resource::host_copy(texture).expect("a host texture");
        let list = host.new_list().unwrap();
        const PIXEL_SHADER_RESOURCE: u32 = 0x80;
        let _ = list.transition(texture, PIXEL_SHADER_RESOURCE);
        evict_now(texture);
        assert!(
            !copy.residency.is_resident(),
            "evicted after the list named it"
        );
        list.close();
        host.execute(&[&list]);
        assert!(
            copy.residency.is_resident(),
            "made resident before the list ran"
        );
        // SAFETY: a plain query.
        assert!(unsafe { host.device.GetDeviceRemovedReason() }.is_ok());
    }

    #[test]
    fn a_heap_slot_viewing_an_evicted_texture_is_null_until_made_again() {
        if crate::host::host().is_none() || !super::available() {
            return;
        }
        let null = std::ptr::null();
        let heap_desc: [u32; 8] = [0, 4, 1, 0, 0, 0, 0, 0];
        let mut heap = null;
        let iid = crate::object::guid(
            0x8efb471d,
            0x616c,
            0x4f49,
            [0x90, 0xf7, 0x12, 0x7b, 0xb7, 0x63, 0xfa, 0x51],
        );
        assert_eq!(
            crate::descriptor::create_heap(null, heap_desc.as_ptr().cast(), &iid, &mut heap),
            0
        );
        let title_heap = crate::descriptor::heap(heap);
        let memory = vec![0u8; 32 << 20];
        let address = memory.as_ptr() as u64;
        let words: [u32; 14] = [
            3,
            0,
            0,
            0,
            4096,
            0,
            2048,
            1 | 1 << 16,
            28,
            1,
            0,
            0x100,
            0,
            0,
        ];
        let mut view = [0u32; 16];
        view[..6].copy_from_slice(&[28, 4, 0x1688, 0, 0, 1]);
        (view[10], view[11]) = (address as u32, (address >> 32) as u32);
        crate::descriptor::create_placed_srv(null, &words, &view, title_heap.start);
        let heap_host = title_heap.host().expect("a host view heap");
        assert!(heap_host.mirror(0, title_heap.start, true).is_some());
        assert!(heap_host.kept(0), "the slot views the texture");
        let texture = crate::resource::texture_at(address, &words).expect("its resource");
        let copy = crate::resource::made_copy(texture).expect("made for the view");
        assert!(copy.committed());
        evict_now(texture);
        assert!(!copy.residency.is_resident(), "evicted");
        assert!(!heap_host.kept(0), "the slot was left null first");
        // A draw reads it: the slot is made again, and its texture resident.
        assert!(heap_host.mirror(0, title_heap.start, true).is_some());
        assert!(heap_host.kept(0), "made again");
        assert!(
            copy.residency.is_resident(),
            "resident before the view was made"
        );
    }
}
