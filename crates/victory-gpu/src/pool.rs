//! Host memory for the textures the title fills, and for their staging
//! buffers, cut from blocks made once.
//!
//! Each such texture had an allocation of its own and its staging buffer
//! another. On an RX 7900 XTX an allocation takes about 310 microseconds
//! whatever its size, on one thread (`texture::allocation_costs`), and a
//! texture cut from a block 6: the pass that makes the textures shaders
//! pick by index (`draw.rs` `heap_textures`) got through 350-500 of the
//! 24,000 queued when the world loads in each of its 250 ms turns, some
//! forty seconds of a world without its textures.
//! `VICTORY_TEXTURE_POOL=0` makes every one an allocation of its own again.
//!
//! A piece given back is used again only two seconds later (nothing the card
//! was told before that is still running: Windows resets a card that takes
//! that long over one submission), and by then no list names it (the owners' rule, as for allocations of their own): a
//! texture's piece comes back when the last reference to the host texture
//! goes (an object of ours the texture holds says so), a staging buffer's
//! when the last list that recorded its copy is reset.

use std::collections::{BTreeMap, VecDeque};
use std::ffi::c_void;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};

use windows::Win32::Graphics::Direct3D12::*;
use windows::core::{GUID, HRESULT, IUnknown, Interface};

/// Bytes of a block; pieces of more than half of one are not pooled (with
/// blocks made ahead, [`ahead`]: of more than one).
const BLOCK: u64 = 64 << 20;
/// Blocks with nothing in them that are kept.
const SPARE: usize = 2;
/// The same while the card has room ([`room`]): 4 GiB of them.
const SPARE_KEPT: usize = 64;
/// The same of blocks of staging buffers, which are the PC's memory: run
/// crash-1 held 5.2 GB of them.
const STAGING_KEPT: usize = 8;
/// How many are kept now.
static SPARES: AtomicUsize = AtomicUsize::new(SPARE);

/// Sets how many empty blocks stay, by the card's memory (budget and use):
/// scaled by local VRAM (`3` on `<= 9 GiB`, `8` on `<= 12 GiB`, `SPARE_KEPT`
/// above) below 85 % ([`crate::host::keep_freed`]), else [`SPARE`].
/// Asked once a second.
pub(crate) fn room(memory: Option<[u64; 4]>) {
    if !crate::residency::on() {
        SPARES.store(spares(memory, crate::host::keep_freed()), Ordering::Relaxed);
        return;
    }
    let pressure = crate::residency::pressured();
    let limit = spares(memory, crate::host::keep_freed() && !pressure);
    let texture_limit = texture_spares(memory, limit, spare_scale());
    SPARES.store(texture_limit, Ordering::Relaxed);
    let tick = now();
    if let Ok(mut pool) = TEXTURES.try_lock() {
        pool.reclaim(tick, texture_limit, true);
    }
    if let Ok(mut pool) = STAGING.try_lock() {
        let staging_limit = if crate::residency::critical_commit() {
            0
        } else if pressure {
            1
        } else if crate::residency::limited_host_ram() {
            limit.min(2)
        } else {
            limit.min(STAGING_KEPT)
        };
        pool.reclaim(tick, staging_limit, true);
    }
}

/// `VICTORY_SPARE_SCALE=1` caps empty texture blocks by the DXGI budget.
fn spare_scale() -> bool {
    static SCALE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *SCALE.get_or_init(|| std::env::var_os("VICTORY_SPARE_SCALE").is_some_and(|v| v == "1"))
}

fn texture_spares(memory: Option<[u64; 4]>, limit: usize, scale: bool) -> usize {
    match memory {
        Some([budget, ..]) if scale => {
            limit.min((budget / 32 / BLOCK).clamp(SPARE as u64, SPARE_KEPT as u64) as usize)
        }
        _ => limit,
    }
}

fn spares(memory: Option<[u64; 4]>, keep: bool) -> usize {
    const GIB: u64 = 1 << 30;
    match memory {
        Some([budget, used, ..]) if keep && budget > 0 && u128::from(used) * 100 < u128::from(budget) * 85 => {
            if budget <= 9 * GIB {
                3
            } else if budget <= 12 * GIB {
                8
            } else {
                SPARE_KEPT
            }
        }
        _ => SPARE,
    }
}

/// Milliseconds until a piece given back is used again. (It was three
/// frames, a tenth of a second while the title shows a loading screen at 30
/// a second, with up to 18 submissions in the air.)
const REST: u64 = 2500;

fn now() -> u64 {
    crate::host::now_ms()
}

pub(crate) fn on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VICTORY_TEXTURE_POOL").is_none_or(|v| v != "0"))
}

/// How many blocks of textures are made ahead, so that one has been there
/// for a while when its first piece is taken (`VICTORY_RIPE_BLOCKS`, 12;
/// 0: none, and new blocks are used at once, as before).
///
/// A page fault of lists already on the card, in a texture made the same
/// tenth of a second as the first piece of a block made for it, lost the
/// device in runs n08 and n11 (2026-11-25): the title gives a slot that
/// those lists read by index a new texture. Why the card does not have the
/// new block's addresses yet is not known (n11 waited for `MakeResident`;
/// `indirect::tests::new_blocks_under_readers` never loses it). A block that
/// every submission begun before its making has left behind ([`Ripe`]) has
/// no such lists: pieces are cut from those first.
fn default_ripe_blocks(vram_budget: Option<u64>) -> usize {
    const GIB: u64 = 1 << 30;
    match vram_budget {
        Some(budget) if budget <= 9 * GIB => 2,
        Some(budget) if budget <= 12 * GIB => 4,
        _ => 12,
    }
}

fn ripe_blocks_for(override_val: Option<usize>, vram_budget: Option<u64>) -> usize {
    override_val.unwrap_or_else(|| default_ripe_blocks(vram_budget))
}

fn ahead() -> usize {
    static OVERRIDE: std::sync::OnceLock<Option<usize>> = std::sync::OnceLock::new();
    let override_val = *OVERRIDE.get_or_init(|| {
        std::env::var("VICTORY_RIPE_BLOCKS")
            .ok()
            .and_then(|v| v.trim().parse().ok())
    });
    let budget = crate::host::host()
        .and_then(|h| h.video_memory())
        .map(|[budget, ..]| budget);
    ripe_blocks_for(override_val, budget)
}

/// Submissions begun and finished ([`crate::host::submissions`]), and how
/// many blocks to make ahead (0: every block is ripe at once).
#[derive(Clone, Copy)]
struct Ripe {
    started: u64,
    finished: u64,
    ahead: usize,
}

impl Ripe {
    const ALL: Ripe = Ripe { started: 0, finished: 0, ahead: 0 };

    fn now() -> Ripe {
        let (started, finished) = crate::host::submissions();
        // Not more than are kept ([`room`]): they would go and be made again.
        Ripe { started, finished, ahead: ahead().min(SPARES.load(Ordering::Relaxed)) }
    }

    fn is<B>(self, block: &Block<B>) -> bool {
        self.ahead == 0 || block.made <= self.finished
    }
}

/// Pieces cut from blocks that were not ripe, for want of a ripe one.
static UNRIPE: AtomicU64 = AtomicU64::new(0);

/// The free ranges of a block: start and length, never two that touch.
struct Ranges(BTreeMap<u64, u64>);

impl Ranges {
    fn new(size: u64) -> Ranges {
        Ranges(BTreeMap::from([(0, size)]))
    }

    /// The lowest place for `len` bytes at a multiple of `align`.
    fn take(&mut self, len: u64, align: u64) -> Option<u64> {
        let (start, size, at) = self.0.iter().find_map(|(&start, &size)| {
            let at = start.next_multiple_of(align);
            (at + len <= start + size).then_some((start, size, at))
        })?;
        self.0.remove(&start);
        if at > start {
            self.0.insert(start, at - start);
        }
        if at + len < start + size {
            self.0.insert(at + len, start + size - at - len);
        }
        Some(at)
    }

    fn give(&mut self, mut start: u64, mut len: u64) {
        if let Some((&before, &size)) = self.0.range(..start).next_back()
            && before + size == start
        {
            self.0.remove(&before);
            (start, len) = (before, len + size);
        }
        if let Some(size) = self.0.remove(&(start + len)) {
            len += size;
        }
        self.0.insert(start, len);
    }

    fn whole(&self, size: u64) -> bool {
        self.0.get(&0) == Some(&size)
    }
}

/// A part of a block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Piece {
    block: usize,
    start: u64,
    len: u64,
}

struct Block<B> {
    memory: B,
    free: Ranges,
    /// The end of the highest piece ever taken: above it the memory is as
    /// it was made (zeros).
    used_to: u64,
    /// The newest submission begun when it was made ([`Ripe`]).
    made: u64,
    /// When that was (ms).
    #[cfg_attr(not(feature = "diagnostics"), allow(dead_code))]
    since: u64,
}

/// Cooldown after a failed block allocation before retrying growth (ms).
const GROWTH_BACKOFF_MS: u64 = 500;

struct Pool<B> {
    /// `None`: a block let go, its place to be used again.
    blocks: Vec<Option<Block<B>>>,
    /// Pieces given back, and when each may be used again.
    resting: VecDeque<(u64, Piece)>,
    /// The empty blocks it was last told to keep, and the most it keeps.
    spare: usize,
    most: usize,
    /// Timestamp before which another failed block allocation will not be retried.
    retry_growth_at: u64,
    /// Blocks being made ahead by a thread outside the lock ([`take_from`]),
    /// counted as empty ones so that no two threads make the same one.
    making_ahead: usize,
}

impl<B: Clone> Pool<B> {
    const fn new(most: usize) -> Pool<B> {
        Pool { blocks: Vec::new(), resting: VecDeque::new(), spare: 0, most, retry_growth_at: 0, making_ahead: 0 }
    }

    /// A piece of `len` bytes and the memory it lies in, and whether that
    /// part was ever used before; `make` makes a block when none has room.
    /// (`make` runs with the pool borrowed; the pools of the process are
    /// taken from with [`take_from`], which makes blocks with their lock let go.)
    #[cfg(test)]
    fn take(&mut self, len: u64, align: u64, now: u64, make: impl FnMut() -> Option<B>) -> Option<(Piece, B, bool)> {
        self.take_ripe(len, align, now, Ripe::ALL, make)
    }

    /// The same from a block that is ripe, if one has room; `make` makes one
    /// ahead too, when few are empty.
    #[cfg(test)]
    fn take_ripe(
        &mut self,
        len: u64,
        align: u64,
        now: u64,
        ripe: Ripe,
        mut make: impl FnMut() -> Option<B>,
    ) -> Option<(Piece, B, bool)> {
        let (i, start) = match self.cut(len, align, now, ripe)? {
            Some(found) => found,
            None => {
                if !self.may_grow(now) {
                    return None;
                }
                let Some(memory) = make() else {
                    self.failed_growth(now);
                    return None;
                };
                self.add_and_cut(memory, ripe.started, len, align)?
            }
        };
        if self.short_ahead(now, ripe) {
            match make() {
                Some(memory) => {
                    self.add(memory, ripe.started);
                }
                None => self.failed_growth(now),
            }
        }
        self.taken(i, start, len, ripe)
    }

    /// Where a piece of `len` bytes was cut from a block there is: `None`
    /// if it is never pooled, `Some(None)` if no block has room.
    fn cut(&mut self, len: u64, align: u64, now: u64, ripe: Ripe) -> Option<Option<(usize, u64)>> {
        // With blocks made ahead, what would be an allocation of its own (and
        // new when the card first reads it) lies in one of those too.
        if len == 0 || len > if ripe.ahead > 0 { BLOCK } else { BLOCK / 2 } {
            return None;
        }
        self.rested(now);
        let mut found = None;
        for (i, block) in self.blocks.iter_mut().enumerate() {
            if let Some(block) = block
                && ripe.is(block)
                && let Some(start) = block.free.take(len, align)
            {
                found = Some((i, start));
                break;
            }
        }
        if found.is_none() && ripe.ahead > 0 {
            // The one that has been there the longest.
            let mut unripe: Vec<(u64, usize)> =
                self.blocks.iter().enumerate().filter_map(|(i, b)| Some((b.as_ref()?.made, i))).collect();
            unripe.sort_unstable();
            found = unripe.into_iter().find_map(|(_, i)| Some((i, self.blocks[i].as_mut()?.free.take(len, align)?)));
        }
        Some(found)
    }

    /// Whether a block may be made now: a failed 64 MiB block must not
    /// trigger another driver call for each small texture. Existing free
    /// ranges remain usable during backoff.
    fn may_grow(&self, now: u64) -> bool {
        now >= self.retry_growth_at
    }

    fn failed_growth(&mut self, now: u64) {
        self.retry_growth_at = now.saturating_add(GROWTH_BACKOFF_MS);
    }

    /// Adds a block just made and cuts the piece from it.
    fn add_and_cut(&mut self, memory: B, made: u64, len: u64, align: u64) -> Option<(usize, u64)> {
        let i = self.add(memory, made);
        Some((i, self.blocks[i].as_mut()?.free.take(len, align)?))
    }

    /// Whether a block is to be made ahead: fewer are empty (or being made)
    /// than are wanted.
    fn short_ahead(&self, now: u64, ripe: Ripe) -> bool {
        ripe.ahead > 0
            && self.may_grow(now)
            && self.blocks.iter().flatten().filter(|b| b.free.whole(BLOCK)).count() + self.making_ahead < ripe.ahead
    }

    /// The piece cut at `start` of block `i`, its memory, and whether it was
    /// used before.
    fn taken(&mut self, i: usize, start: u64, len: u64, ripe: Ripe) -> Option<(Piece, B, bool)> {
        let block = self.blocks[i].as_mut()?;
        if ripe.ahead > 0 && !ripe.is(block) {
            let n = UNRIPE.fetch_add(1, Ordering::Relaxed) + 1;
            if n.is_power_of_two() {
                tracing::info!("textures cut from blocks made while lists ran that still run: {n}");
            }
        }
        let again = start < block.used_to;
        block.used_to = block.used_to.max(start + len);
        Some((Piece { block: i, start, len }, block.memory.clone(), again))
    }

    fn add(&mut self, memory: B, made: u64) -> usize {
        let i = match self.blocks.iter().position(Option::is_none) {
            Some(i) => i,
            None => {
                self.blocks.push(None);
                self.blocks.len() - 1
            }
        };
        self.blocks[i] = Some(Block { memory, free: Ranges::new(BLOCK), used_to: 0, made, since: crate::host::now_ms() });
        i
    }

    fn give(&mut self, piece: Piece, now: u64) {
        self.resting.push_back((now + REST, piece));
    }

    /// Makes the pieces that rested long enough free, and lets go of empty
    /// blocks past [`SPARE`].
    fn rested(&mut self, now: u64) {
        self.reclaim(now, SPARES.load(Ordering::Relaxed), false);
    }

    /// [`Pool::rested`] with at most `limit` empty blocks kept; `sweep`: the
    /// empty blocks are looked at even if no piece came back.
    fn reclaim(&mut self, now: u64, limit: usize, sweep: bool) {
        let mut any = false;
        while let Some(&(ready, piece)) = self.resting.front()
            && ready <= now
        {
            self.resting.pop_front();
            if let Some(block) = self.blocks[piece.block].as_mut() {
                block.free.give(piece.start, piece.len);
                any |= block.free.whole(BLOCK);
            }
        }
        // (Without a piece given back too, once there is less room.)
        let most = limit.min(self.most);
        if sweep || any || most < self.spare {
            self.spare = most;
            let mut spare = 0;
            for block in &mut self.blocks {
                if block.as_ref().is_some_and(|b| b.free.whole(BLOCK)) {
                    spare += 1;
                    if spare > most {
                        *block = None;
                    }
                }
            }
        }
    }

    fn blocks(&self) -> usize {
        self.blocks.iter().flatten().count()
    }
}

/// A mapped upload buffer's memory.
#[derive(Clone)]
struct Upload(ID3D12Resource, *mut u8);
// SAFETY: the mapping is written by the owners of its pieces, each its own.
unsafe impl Send for Upload {}

static TEXTURES: Mutex<Pool<ID3D12Heap>> = Mutex::new(Pool::new(SPARE_KEPT));
static STAGING: Mutex<Pool<Upload>> = Mutex::new(Pool::new(STAGING_KEPT));

fn lock<T>(m: &'static Mutex<T>) -> std::sync::MutexGuard<'static, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// [`Pool::take_ripe`] of a pool behind a lock, with blocks made while the
/// lock is let go. Making one is a 64 MiB heap or upload buffer, made
/// resident and waited for until it is mapped (`crate::host::resident`,
/// `crate::host::mapped`): a tenth of a millisecond to some, but over a
/// second near the card's budget (`MakeResident` waits for the system to
/// evict first). Under the lock every thread that took a texture, or gave
/// one back, waited for that.
///
/// `ripe` is asked again once a block is made: a block is ripe once the
/// submissions begun before it was made have finished, so its mark is
/// taken after the making (a mark taken before would be too low).
///
/// Two threads that both find no room may each make a block; the second
/// is a block made ahead, or a spare that [`Pool::reclaim`] lets go.
fn take_from<B: Clone + 'static>(
    pool: &'static Mutex<Pool<B>>,
    len: u64,
    align: u64,
    ripe: impl Fn() -> Ripe,
    mut make: impl FnMut() -> Option<B>,
) -> Option<(Piece, B, bool)> {
    let first = ripe();
    let mut p = lock(pool);
    let (i, start) = match p.cut(len, align, now(), first)? {
        Some(found) => found,
        None => {
            if !p.may_grow(now()) {
                return None;
            }
            drop(p);
            let made = make();
            let mark = ripe().started;
            p = lock(pool);
            let Some(memory) = made else {
                p.failed_growth(now());
                return None;
            };
            p.add_and_cut(memory, mark, len, align)?
        }
    };
    let ahead = p.short_ahead(now(), first);
    if ahead {
        p.making_ahead += 1;
    }
    let taken = p.taken(i, start, len, first);
    drop(p);
    if ahead {
        /// Counts the block as made (or not) even if `make` unwinds.
        struct Making<B: Clone + 'static>(&'static Mutex<Pool<B>>);
        impl<B: Clone + 'static> Drop for Making<B> {
            fn drop(&mut self) {
                let mut p = lock(self.0);
                p.making_ahead = p.making_ahead.saturating_sub(1);
            }
        }
        let making = Making(pool);
        let made = make();
        let mark = ripe().started;
        let mut p = lock(pool);
        // Counted off under the same lock the block is added under.
        std::mem::forget(making);
        p.making_ahead = p.making_ahead.saturating_sub(1);
        match made {
            Some(memory) => {
                p.add(memory, mark);
            }
            None => p.failed_growth(now()),
        }
    }
    taken
}

/// Bytes of the pieces textures hold now.
static HELD: AtomicU64 = AtomicU64::new(0);

/// Bytes of the texture blocks no texture holds: the card's memory in use
/// by us that new textures take first.
pub(crate) fn texture_room() -> u64 {
    (lock(&TEXTURES).blocks() as u64 * BLOCK).saturating_sub(HELD.load(Ordering::Relaxed))
}

/// For the memory line: blocks of textures and of staging buffers, in MiB.
#[cfg(feature = "diagnostics")]
pub(crate) fn memory() -> [u64; 2] {
    [lock(&TEXTURES).blocks() as u64 * (BLOCK >> 20), lock(&STAGING).blocks() as u64 * (BLOCK >> 20)]
}

/// Part of a block of mapped upload memory, back in the pool when dropped.
pub(crate) struct StagingPiece(Piece);

impl Drop for StagingPiece {
    fn drop(&mut self) {
        lock(&STAGING).give(self.0, now());
    }
}

/// `bytes` of upload memory, all zeros: the buffer it lies in, where in
/// that buffer, and its mapping. `None`: not pooled.
pub(crate) fn staging(host: &crate::host::Host, bytes: u64) -> Option<(StagingPiece, ID3D12Resource, u64, *mut u8)> {
    if !on() {
        return None;
    }
    // Copies from a buffer begin at multiples of 512.
    let bytes = bytes.next_multiple_of(512);
    let (piece, memory, again) = take_from(&STAGING, bytes, 512, || Ripe::ALL, || {
        // (`VICTORY_RESIDENCY=1`: no new block for a texture picked by index
        // while commit is short; those a draw samples now still get one.)
        let speculative = crate::texture::PICKING.get().is_some() && !crate::texture::BAKING.get();
        if speculative && !crate::residency::allow_staging_growth(false) {
            return None;
        }
        let (buffer, data) = host.upload_buffer(BLOCK)?;
        crate::host::name(&buffer, || "staging of textures (a block)".into());
        Some(Upload(buffer, data))
    })?;
    let owned = StagingPiece(piece);
    // SAFETY: the piece lies in the block's mapping.
    let data = unsafe { memory.1.add(piece.start as usize) };
    if again {
        // What is not detiled into it (an unmapped texture) uploads zeros,
        // as from a buffer of its own.
        // SAFETY: the piece is ours alone.
        unsafe { std::ptr::write_bytes(data, 0, bytes as usize) };
    }
    Some((owned, memory.0, piece.start, data))
}

/// An object the host texture holds and releases when it goes: its piece
/// is free then.
#[repr(C)]
struct Reclaim {
    methods: &'static [*const (); 3],
    references: AtomicU32,
    piece: Piece,
}

struct Methods([*const (); 3]);
// SAFETY: function addresses only.
unsafe impl Sync for Methods {}

static RECLAIM: Methods = Methods([query as *const (), add_ref as *const (), release as *const ()]);

unsafe extern "system" fn query(this: *mut c_void, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    const E_NOINTERFACE: HRESULT = HRESULT(0x8000_4002_u32 as i32);
    // SAFETY: COM's arguments.
    unsafe {
        if out.is_null() {
            return HRESULT(0x8000_4003_u32 as i32);
        }
        if iid.as_ref().is_some_and(|iid| *iid == IUnknown::IID) {
            add_ref(this);
            *out = this;
            return HRESULT(0);
        }
        *out = std::ptr::null_mut();
    }
    E_NOINTERFACE
}

unsafe extern "system" fn add_ref(this: *mut c_void) -> u32 {
    // SAFETY: one of ours, alive while a reference to it is.
    unsafe { (*this.cast::<Reclaim>()).references.fetch_add(1, Ordering::Relaxed) + 1 }
}

unsafe extern "system" fn release(this: *mut c_void) -> u32 {
    // SAFETY: as above; the last reference frees the box it was made as.
    unsafe {
        let left = (*this.cast::<Reclaim>()).references.fetch_sub(1, Ordering::AcqRel) - 1;
        if left == 0 {
            let gone = Box::from_raw(this.cast::<Reclaim>());
            HELD.fetch_sub(gone.piece.len, Ordering::Relaxed);
            lock(&TEXTURES).give(gone.piece, now());
        }
        left
    }
}

/// Textures made in the last second ([`fresh`]), and when.
static RECENT: Mutex<VecDeque<(usize, u64)>> = Mutex::new(VecDeque::new());

/// Whether `texture` was made in a block in the last second.
pub(crate) fn fresh(texture: &ID3D12Resource) -> bool {
    let raw = texture.as_raw() as usize;
    let since = now().saturating_sub(1000);
    RECENT.lock().unwrap_or_else(|e| e.into_inner()).iter().rev().take_while(|r| r.1 >= since).any(|r| r.0 == raw)
}

fn made_now(texture: &ID3D12Resource) {
    let now = now();
    let mut recent = RECENT.lock().unwrap_or_else(|e| e.into_inner());
    while recent.front().is_some_and(|r| r.1 + 1000 < now) {
        recent.pop_front();
    }
    recent.push_back((texture.as_raw() as usize, now));
}

const RECLAIM_KEY: GUID = GUID::from_u128(0x6f0e2f0c_5a59_4b1e_9a3f_7f4d2c9be713);

/// Whether `texture` was placed inside a pool block (so D3D12 aliasing
/// barriers are legal on it; committed fallback textures > `TEXTURE_BLOCK`
/// must not receive aliasing barriers or `Close()` fails with `E_INVALIDARG`).
pub(crate) fn is_pooled(texture: &ID3D12Resource) -> bool {
    if !on() {
        return false;
    }
    let mut size = 0u32;
    // SAFETY: querying presence with `None` writes only `size` and does not AddRef.
    unsafe { texture.GetPrivateData(&RECLAIM_KEY, &mut size, None).is_ok() }
}

/// Makes `texture` give `piece` back when the last reference to it goes.
fn give_back_with(texture: &ID3D12Resource, piece: Piece) {
    let made = Box::into_raw(Box::new(Reclaim { methods: &RECLAIM.0, references: AtomicU32::new(1), piece }));
    // SAFETY: a COM object of ours with one reference, which this takes and
    // drops at the end; the texture takes its own.
    unsafe {
        let object = IUnknown::from_raw(made.cast());
        if texture.SetPrivateDataInterface(&RECLAIM_KEY, &object).is_err() {
            // Never given back: the piece stays taken, nothing is reused
            // under a texture that may be alive.
            std::mem::forget(object);
        }
    }
}

/// A texture of `desc` (no target, depth or unordered access) in a block;
/// `None`: not pooled, or the host refused.
///
/// # Safety
/// `desc` is a valid description.
pub(crate) unsafe fn texture(
    device: &ID3D12Device,
    desc: *const D3D12_RESOURCE_DESC,
    state: D3D12_RESOURCE_STATES,
) -> Option<ID3D12Resource> {
    if !on() {
        return None;
    }
    // Small textures lie at multiples of 4 KiB where the host allows it
    // (the top mip is 64 KiB at most): 64 KiB each otherwise.
    // SAFETY: the caller's description.
    let mut d = unsafe { *desc };
    d.Alignment = u64::from(D3D12_SMALL_RESOURCE_PLACEMENT_ALIGNMENT);
    // SAFETY: plain queries.
    let mut info = unsafe { device.GetResourceAllocationInfo(0, &[d]) };
    if info.SizeInBytes == u64::MAX || info.Alignment != d.Alignment {
        d.Alignment = 0;
        info = unsafe { device.GetResourceAllocationInfo(0, &[d]) };
    }
    if info.SizeInBytes == u64::MAX {
        return None;
    }
    #[cfg_attr(not(feature = "diagnostics"), allow(unused_variables))]
    let (piece, heap, again) = take_from(&TEXTURES, info.SizeInBytes, info.Alignment.max(1), Ripe::now, || {
        let desc = D3D12_HEAP_DESC {
            SizeInBytes: BLOCK,
            Properties: D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, ..Default::default() },
            Alignment: 0,
            Flags: D3D12_HEAP_FLAG_ALLOW_ONLY_NON_RT_DS_TEXTURES | crate::host::not_resident(),
        };
        let mut heap: Option<ID3D12Heap> = None;
        // SAFETY: plain creation.
        if let Err(e) = unsafe { device.CreateHeap(&desc, &mut heap) } {
            crate::residency::allocation_error(&e);
            tracing::warn!(error = %e, "no block of texture memory");
        }
        if let Some(h) = &heap
            && let Err(e) = crate::host::resident(device, h).and_then(|()| crate::host::mapped(device))
        {
            tracing::warn!(error = %e, "a block of texture memory is not resident");
            return None;
        }
        heap
    })?;
    let mut made: Option<ID3D12Resource> = None;
    // SAFETY: the piece is no one else's; the heap lives while a piece of
    // it is taken.
    let result = unsafe { device.CreatePlacedResource(&heap, piece.start, &d, state, None, &mut made) };
    match (result, made) {
        (Ok(()), Some(made)) => {
            HELD.fetch_add(piece.len, Ordering::Relaxed);
            give_back_with(&made, piece);
            made_now(&made);
            #[cfg(feature = "diagnostics")]
            {
                crate::book::made(&made, || "a texture in a block".into());
                crate::book::block(&heap, &made, piece.start, BLOCK);
                crate::book::note(made.as_raw() as usize, || {
                    let block = lock(&TEXTURES).blocks.get(piece.block).and_then(Option::as_ref).map(|b| (b.made, b.since));
                    let (submission, since) = block.unwrap_or_default();
                    format!(
                        "; piece {:#x}+{:#x} of block {} {:#x} made {:.1} s before it at submission {submission}, used before: {again}, made at submissions {:?}",
                        piece.start,
                        piece.len,
                        piece.block,
                        heap.as_raw() as usize,
                        now().saturating_sub(since) as f64 / 1e3,
                        crate::host::submissions()
                    )
                });
            }
            Some(made)
        }
        _ => {
            lock(&TEXTURES).give(piece, now());
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_are_cut_aligned_and_join_again() {
        let mut r = Ranges::new(1024);
        assert_eq!(r.take(100, 64), Some(0));
        assert_eq!(r.take(100, 64), Some(128));
        assert_eq!(r.take(28, 4), Some(100));
        assert_eq!(r.take(2000, 4), None);
        r.give(128, 100);
        r.give(0, 100);
        assert!(!r.whole(1024));
        r.give(100, 28);
        assert!(r.whole(1024));
    }

    #[test]
    fn empty_blocks_stay_while_the_card_has_room() {
        const GIB: u64 = 1 << 30;
        assert_eq!(spares(Some([24 * GIB, 20 * GIB, 0, 0]), true), SPARE_KEPT);
        assert_eq!(spares(Some([12 * GIB, 9 * GIB, 0, 0]), true), 8);
        assert_eq!(spares(Some([8 * GIB, 6 * GIB, 0, 0]), true), 3);
        assert_eq!(spares(Some([24 * GIB, 21 * GIB, 0, 0]), true), SPARE);
        assert_eq!(spares(Some([8 * GIB, 7 * GIB, 0, 0]), true), SPARE);
        assert_eq!(spares(Some([24 * GIB, 2 * GIB, 0, 0]), false), SPARE);
        assert_eq!(spares(None, true), SPARE);

        // With room: our tiers by the budget (3, 8 or SPARE_KEPT), and with
        // the scale switch at most a thirty-second of the budget as well.
        let budgets = [
            (1u64, 3, SPARE),
            (8, 3, 3),
            (12, 8, 6),
            (16, SPARE_KEPT, 8),
            (24, SPARE_KEPT, 12),
            (256, SPARE_KEPT, SPARE_KEPT),
        ];
        for (gib, tier, scaled) in budgets {
            let budget = gib << 30;
            for (percent, legacy, expected) in [
                (84, tier, scaled),
                (85, SPARE, SPARE),
                (86, SPARE, SPARE),
            ] {
                let memory = Some([budget, (budget * percent).div_ceil(100), 0, 0]);
                let limit = spares(memory, true);
                for (scale, expected) in [(false, legacy), (true, expected)] {
                    assert_eq!(
                        texture_spares(memory, limit, scale),
                        expected,
                        "{gib} GiB at {percent}%, scale={scale}"
                    );
                }
            }
        }

        let memory = Some([8 << 30, 1 << 30, 0, 0]);
        assert_eq!(texture_spares(memory, spares(memory, false), true), SPARE);
        assert_eq!(texture_spares(None, spares(None, true), true), SPARE);
    }

    #[test]
    fn ripe_blocks_scale_with_vram_and_honor_override() {
        const GIB: u64 = 1 << 30;
        assert_eq!(ripe_blocks_for(None, Some(8 * GIB)), 2);
        assert_eq!(ripe_blocks_for(None, Some(12 * GIB)), 4);
        assert_eq!(ripe_blocks_for(None, Some(24 * GIB)), 12);
        assert_eq!(ripe_blocks_for(None, None), 12);
        assert_eq!(ripe_blocks_for(Some(0), Some(8 * GIB)), 0);
        assert_eq!(ripe_blocks_for(Some(6), Some(8 * GIB)), 6);
    }

    #[test]
    fn pieces_never_overlap_and_rest_before_they_are_used_again() {
        let mut pool: Pool<u32> = Pool::new(SPARE);
        let made = std::cell::Cell::new(0);
        let make = || {
            made.set(made.get() + 1);
            Some(made.get())
        };
        let mut taken: Vec<Piece> = Vec::new();
        for i in 0..500u64 {
            let (p, _, again) = pool.take(65536 * (1 + i % 7), 65536, 0, make).unwrap();
            assert!(!again);
            assert!(taken.iter().all(|q| q.block != p.block || p.start + p.len <= q.start || q.start + q.len <= p.start));
            taken.push(p);
        }
        let blocks = pool.blocks();
        assert!(blocks >= 2);
        // Given back at 10: not used again before 10 + REST.
        let first = taken[0];
        pool.give(first, 10);
        let (p, _, _) = pool.take(first.len, 65536, 10 + REST - 1, make).unwrap();
        assert_ne!((p.block, p.start), (first.block, first.start));
        let (q, _, again) = pool.take(first.len, 65536, 10 + REST, make).unwrap();
        assert_eq!((q.block, q.start, again), (first.block, first.start, true));
        // Everything back: the empty blocks past the spare ones go.
        for p in taken.drain(1..).chain([p, q]) {
            pool.give(p, 20);
        }
        pool.rested(20 + REST);
        assert_eq!(pool.blocks(), blocks.min(SPARE));
        // Too big for a block, and a block that cannot be made.
        assert!(pool.take(BLOCK, 65536, 30, make).is_none());
        let mut none: Pool<u32> = Pool::new(SPARE);
        assert!(none.take(65536, 65536, 0, || None).is_none());
    }

    /// A block is made with the pool's lock let go (making one waits for
    /// residency, up to a second near the card's budget), it is marked with
    /// the submissions begun after its making, and a block made ahead is
    /// counted off again whether it was made or not.
    #[test]
    fn blocks_are_made_with_the_pool_let_go() {
        static POOL: Mutex<Pool<u64>> = Mutex::new(Pool::new(SPARE_KEPT));
        let made = std::cell::Cell::new(0u64);
        let make = || {
            assert!(POOL.try_lock().is_ok(), "a block was made with the pool's lock held");
            made.set(made.get() + 1);
            Some(made.get())
        };
        // Ten more submissions begun for every block made: a mark taken
        // before a making would be ten lower.
        let ripe = || Ripe { started: made.get() * 10, finished: 0, ahead: 2 };
        let (piece, memory, again) = take_from(&POOL, BLOCK / 2, 65536, ripe, make).unwrap();
        assert_eq!((piece.start, memory, again), (0, 1, false));
        assert_eq!(made.get(), 2, "one more made ahead");
        {
            let p = lock(&POOL);
            let marks: Vec<u64> = p.blocks.iter().flatten().map(|b| b.made).collect();
            assert_eq!(marks, [10, 20]);
            assert_eq!(p.making_ahead, 0);
        }
        // Block 2 is empty, so one more is made ahead; then that one fails.
        assert_eq!(take_from(&POOL, BLOCK / 2, 65536, ripe, make).unwrap().1, 1);
        assert_eq!(made.get(), 3);
        let refuse = || {
            assert!(POOL.try_lock().is_ok(), "a block was made with the pool's lock held");
            None
        };
        let full = || Ripe { started: 100, finished: 100, ahead: 4 };
        assert!(take_from(&POOL, BLOCK / 2, 65536, full, refuse).is_some(), "cut from a block there is");
        let p = lock(&POOL);
        assert_eq!(p.making_ahead, 0, "a block not made is counted off");
        assert!(!p.may_grow(now()), "and blocks are not asked for again at once");
    }

    #[test]
    fn pieces_come_from_blocks_older_than_the_lists_that_run() {
        let mut pool: Pool<u64> = Pool::new(SPARE_KEPT);
        let made = std::cell::Cell::new(0);
        let make = || {
            made.set(made.get() + 1);
            Some(made.get())
        };
        let at = |started, finished| Ripe { started, finished, ahead: 2 };
        let take = |pool: &mut Pool<u64>, ripe| pool.take_ripe(BLOCK / 2, 65536, 0, ripe, make).unwrap().1;
        // Nothing runs: at once, and two are made ahead, one a time.
        assert_eq!(take(&mut pool, at(0, 0)), 1);
        assert_eq!(made.get(), 2);
        assert_eq!(take(&mut pool, at(0, 0)), 1);
        assert_eq!(made.get(), 3);
        // 4 is made while submission 5 runs.
        assert_eq!(take(&mut pool, at(5, 4)), 2);
        assert_eq!(made.get(), 4);
        assert_eq!(take(&mut pool, at(5, 4)), 2);
        assert_eq!(take(&mut pool, at(6, 4)), 3);
        assert_eq!(take(&mut pool, at(6, 4)), 3);
        // None is ripe: the oldest; it is ripe once 5 has finished.
        assert_eq!(take(&mut pool, at(7, 4)), 4);
        assert_eq!(take(&mut pool, at(7, 5)), 4);
        // A whole block's worth too.
        assert_eq!(pool.take_ripe(BLOCK, 65536, 0, at(8, 7), make).unwrap().1, 5);
        // Off: the first with room, whatever its age, none ahead.
        let n = made.get();
        assert_eq!(pool.take(BLOCK / 2, 65536, 0, make).unwrap().1, 6);
        assert_eq!(made.get(), n);
        assert!(pool.take(BLOCK, 65536, 0, make).is_none());
    }

    /// A pooled texture's piece comes back when its last reference goes, not
    /// before; its staging piece begins as zeros.
    #[test]
    fn a_texture_gives_its_piece_back_when_it_goes() {
        let Some(host) = crate::host::host() else { return };
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Width: 256,
            Height: 256,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM,
            SampleDesc: windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            ..Default::default()
        };
        // SAFETY: a valid description.
        let Some(t) = (unsafe { texture(&host.device, &desc, D3D12_RESOURCE_STATE_COMMON) }) else { return };
        assert_eq!(crate::host::references(&t), 1, "what the texture holds is no reference to it");
        let resting = || lock(&TEXTURES).resting.len();
        let before = resting();
        let other = t.clone();
        drop(t);
        assert_eq!(resting(), before);
        drop(other);
        assert_eq!(resting(), before + 1);
        // SAFETY: the piece's mapping, 4096 bytes of it ours.
        unsafe {
            let (piece, _, at, data) = staging(host, 4096).unwrap();
            assert!(at.is_multiple_of(512));
            std::ptr::write_bytes(data, 0xAB, 4096);
            drop(piece);
            lock(&STAGING).rested(u64::MAX);
            let (_piece, _, _, data) = staging(host, 4096).unwrap();
            assert!((0..4096).all(|i| *data.add(i) == 0));
            // SAFETY: plain query.
            assert!(host.device.GetDeviceRemovedReason().is_ok());
        }
    }

    #[test]
    fn pool_growth_backs_off_after_refusal() {
        let mut pool: Pool<u32> = Pool::new(4);
        let calls = std::cell::Cell::new(0);
        let allow = std::cell::Cell::new(true);
        let mut next = 1u32;
        let mut make = || {
            calls.set(calls.get() + 1);
            if allow.get() {
                let id = next;
                next += 1;
                Some(id)
            } else {
                None
            }
        };
        // First allocation succeeds and still leaves half the block free.
        let (first, mem, _) = pool.take(BLOCK / 2, 65536, 100, &mut make).unwrap();
        assert_eq!((mem, calls.get()), (1, 1));
        allow.set(false);
        // Existing free slice in block 1 satisfies another half-block during/before backoff.
        assert_eq!(pool.take(BLOCK / 2, 65536, 100, &mut make).unwrap().1, 1);
        // Block 1 is now full; refusal arms 500 ms backoff.
        assert!(pool.take(BLOCK / 2, 65536, 100, &mut make).is_none());
        assert_eq!(calls.get(), 2);
        // Calls inside cooldown must not invoke `make` again, while freeing lets existing space work.
        assert!(pool.take(BLOCK / 2, 65536, 100 + GROWTH_BACKOFF_MS - 1, &mut make).is_none());
        assert_eq!(calls.get(), 2);
        pool.give(first, 100);
        assert_eq!(pool.take(BLOCK / 2, 65536, 100 + REST, &mut make).unwrap().1, 1);
        assert_eq!(calls.get(), 2);
        // After cooldown elapses, growth retries and succeeds.
        allow.set(true);
        assert_eq!(pool.take(BLOCK / 2, 65536, 100 + GROWTH_BACKOFF_MS, &mut make).unwrap().1, 2);
        assert_eq!(calls.get(), 3);
    }
}
