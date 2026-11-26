//! A stress test of the write tracking: no write the title made before a
//! submission may be missing from a host copy after that submission's
//! refresh.
//!
//! Title memory of both kinds is made: write-watched memory (the kernel's
//! write watch) and a section mapped twice (two views of the same pages,
//! guarded by us and seen through our fault handler). Host copies over it
//! overlap, as the title's buffers do, and one copy is of each view.
//!
//! - Three title threads write values that only grow (one counter for all)
//!   into 8-byte slots at random, in both kinds of memory and through both
//!   views.
//! - A copy-back thread writes the same way through [`writing`], as ours
//!   does when shaders' bytes come back.
//! - A submitter, per submission: picks a range of each copy, reads what
//!   the title memory holds in it (the moment the submission is given),
//!   makes the submission's one asking ([`ask_once`]), writes a few slots
//!   itself the way a title buffer copy at submission does (`writing`, the
//!   write, [`wrote`]), and refreshes the ranges on four threads in pieces
//!   ([`Tracked::take`], copying what it gives), then ends the asking.
//! - After each submission every slot of its ranges must hold at least the
//!   value read when it was given (and the submitter's own writes): the
//!   values only grow, so anything less is a lost write.
//!
//! Timing is shaken with short sleeps at random points. Submissions:
//! `VICTORY_STRESS_SUBMISSIONS` (400 by default; thousands to look harder).
//!
//! What it does not cover yet: views unmapped and mapped again while
//! copies hold them, placeholders and holes, and the flights' merges.

use super::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Pages (64 KiB) of each kind of memory.
const PAGES: usize = 24;
/// Slots looked at in each page: every 4 KiB, so that every kernel page of
/// the write watch is touched.
const SLOTS: usize = PAGE / 4096;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    /// Now and then a short sleep, to shake the order threads meet in.
    fn shake(&mut self) {
        match self.below(64) {
            0 => std::thread::sleep(std::time::Duration::from_micros(self.below(300) as u64)),
            1..=4 => std::thread::yield_now(),
            _ => {}
        }
    }
}

/// The address of slot `slot` (0..PAGES * SLOTS) of memory at `base`.
fn slot_at(base: usize, slot: usize) -> usize {
    base + (slot / SLOTS) * PAGE + (slot % SLOTS) * 4096 + 8 * (slot % 7)
}

/// A host copy of `[start, start + len)` of title memory.
struct Copy {
    tracked: Tracked,
    start: usize,
    host: std::sync::Mutex<Vec<u64>>,
    /// The ranges the tracking gave in the current submission.
    given: std::sync::Mutex<Vec<(usize, usize)>>,
}

impl Copy {
    fn new(start: usize, len: usize, views: bool) -> Self {
        let tracked = track(start, len, views).expect("tracked");
        Copy { tracked, start, host: std::sync::Mutex::new(vec![0; len / 8]), given: std::sync::Mutex::default() }
    }

    /// The copy's refresh of `[from, to)`: what the tracking gives is copied,
    /// word by word (a byte copy could tear a word being written).
    fn refresh(&self, from: usize, to: usize) {
        let ranges = self.tracked.take(from, to);
        self.given.lock().unwrap().extend_from_slice(&ranges);
        let mut host = self.host.lock().unwrap();
        for (a, b) in ranges {
            let mut at = a & !7;
            while at < b {
                // SAFETY: title memory of this test, mapped for its whole run.
                let v = unsafe { std::ptr::read_volatile(at as *const u64) };
                host[(at - self.start) / 8] = v;
                at += 8;
            }
        }
    }

    fn host_at(&self, at: usize) -> u64 {
        self.host.lock().unwrap()[(at - self.start) / 8]
    }
}

/// Writes `value` into the slot at `at` (title memory of this test) unless
/// it holds more already: a slot's value only grows. (A plain store could
/// put a smaller one over a larger: two writers take 100 and 101 from the
/// counter, the one with 100 faults on a guarded page and lands last.)
fn put(at: usize, value: u64) {
    // SAFETY: an aligned slot of title memory of this test; a guarded page
    // faults into our handler, which opens it and lets the write land.
    unsafe { (*(at as *const std::sync::atomic::AtomicU64)).fetch_max(value, Ordering::AcqRel) };
}

fn get(at: usize) -> u64 {
    // SAFETY: as in `put`.
    unsafe { std::ptr::read_volatile(at as *const u64) }
}

#[test]
fn no_write_before_a_submission_is_lost() {
    const MEM_WRITE_WATCH: u32 = 0x20_0000;
    let submissions: usize =
        std::env::var("VICTORY_STRESS_SUBMISSIONS").ok().and_then(|v| v.parse().ok()).unwrap_or(400);
    let knob = |name: &str, usual: usize| std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(usual);
    // To narrow a failure down: the views written and copied (1: the first
    // only), the copy-back thread (0: none), refresh threads.
    let (views, copy_back, wide) =
        (knob("VICTORY_STRESS_VIEWS", 2), knob("VICTORY_STRESS_COPYBACK", 1), knob("VICTORY_STRESS_WIDE", 4).max(1));
    // 0: each range refreshed in one piece, not cut at random.
    let cut = knob("VICTORY_STRESS_CUT", 1);
    let len = PAGES * PAGE;
    // SAFETY: a fresh write-watched allocation, freed at the end.
    let watched = unsafe {
        windows_sys::Win32::System::Memory::VirtualAlloc(
            std::ptr::null(),
            len,
            windows_sys::Win32::System::Memory::MEM_RESERVE
                | windows_sys::Win32::System::Memory::MEM_COMMIT
                | MEM_WRITE_WATCH,
            windows_sys::Win32::System::Memory::PAGE_READWRITE,
        )
    } as usize;
    assert_ne!(watched, 0);
    let (section, one, two) = tests::two_views(len);

    // The copies: two overlapping over the watched memory, one of each view.
    let half = PAGES / 3 * PAGE;
    let copies = Arc::new(vec![
        Copy::new(watched, 2 * half, false),
        Copy::new(watched + half, len - half, false),
        Copy::new(one, len, true),
        Copy::new(two, len, true),
    ]);
    // Where a slot of each copy's memory is: (base, the copy's own first slot).
    let bases = [watched, watched, one, two];

    let counter = Arc::new(AtomicU64::new(1));
    let stop = Arc::new(AtomicBool::new(false));
    let mut threads = Vec::new();
    // The title's threads.
    for t in 0..3u64 {
        let (counter, stop) = (counter.clone(), stop.clone());
        threads.push(std::thread::spawn(move || {
            let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ (t + 1));
            let bases = [watched, one, two];
            while !stop.load(Ordering::Relaxed) {
                let at = slot_at(bases[rng.below(1 + views)], rng.below(PAGES * SLOTS));
                put(at, counter.fetch_add(1, Ordering::AcqRel));
                rng.shake();
            }
        }));
    }
    // Our copy-back.
    if copy_back != 0 {
        let (counter, stop) = (counter.clone(), stop.clone());
        threads.push(std::thread::spawn(move || {
            let mut rng = Rng(0xD1B5_4A32_D192_ED03);
            let bases = [watched, one, two];
            while !stop.load(Ordering::Relaxed) {
                let at = slot_at(bases[rng.below(1 + views)], rng.below(PAGES * SLOTS));
                writing(at, 8);
                put(at, counter.fetch_add(1, Ordering::AcqRel));
                std::thread::sleep(std::time::Duration::from_micros(50 + rng.below(200) as u64));
            }
        }));
    }

    let mut rng = Rng(0xA076_1D64_78BD_642F);
    let mut lost = Vec::new();
    let mut checked = 0usize;
    for submission in 0..submissions {
        // A range of each copy, in slots of its memory (cut to the copy).
        let mut picked = Vec::new();
        for (c, copy) in copies.iter().enumerate().take(2 + views) {
            let copy_len = copy.host.lock().unwrap().len() * 8;
            let a = copy.start + (rng.below(copy_len / 8) * 8);
            let b = (a + 8 + rng.below(copy_len / 3) / 8 * 8).min(copy.start + copy_len);
            picked.push((c, a, b));
        }
        // The submission is given: what the title memory holds in its ranges.
        let mut expected: Vec<(usize, usize, u64)> = Vec::new();
        for &(c, a, b) in &picked {
            let base = bases[c];
            for slot in 0..PAGES * SLOTS {
                let at = slot_at(base, slot);
                if (a..b).contains(&at) && at + 8 <= b {
                    expected.push((c, at, get(at)));
                }
            }
        }
        rng.shake();
        for copy in copies.iter() {
            copy.given.lock().unwrap().clear();
        }
        let ranges: Vec<(usize, usize)> = picked.iter().map(|&(_, a, b)| (a, b)).collect();
        ask_once(&ranges);
        // A title buffer copy done at submission: told, as the host tells it.
        for _ in 0..rng.below(3) {
            let &(c, a, b) = &picked[rng.below(picked.len())];
            let slots: Vec<usize> =
                (0..PAGES * SLOTS).map(|s| slot_at(bases[c], s)).filter(|&at| at >= a && at + 8 <= b).collect();
            if slots.is_empty() {
                continue;
            }
            let at = slots[rng.below(slots.len())];
            writing(at, 8);
            let v = counter.fetch_add(1, Ordering::AcqRel);
            put(at, v);
            wrote(at, 8);
            expected.push((c, at, v));
        }
        // The refreshes, on four threads, each range in pieces.
        let mut pieces: Vec<(usize, usize, usize)> = Vec::new();
        for &(c, a, b) in &picked {
            let mut at = a;
            while at < b {
                let end = if cut == 0 { b } else { (at + 8 + rng.below(3 * PAGE) / 8 * 8).min(b) };
                pieces.push((c, at, end));
                at = end;
            }
        }
        let seed = rng.next();
        std::thread::scope(|scope| {
            for w in 0..wide {
                let (pieces, copies) = (&pieces, &copies);
                scope.spawn(move || {
                    let mut rng = Rng(seed ^ (w as u64 + 1) * 0x9E37_79B9);
                    for (i, &(c, a, b)) in pieces.iter().enumerate() {
                        if i % wide == w {
                            rng.shake();
                            copies[c].refresh(a, b);
                        }
                    }
                });
            }
        });
        ask_once_end();
        checked += expected.len();
        for &(c, at, want) in &expected {
            let got = copies[c].host_at(at);
            if got < want {
                let given = copies[c].given.lock().unwrap().iter().any(|&(a, b)| a <= at && at + 8 <= b);
                let now = get(at);
                eprintln!(
                    "lost: submission {submission} copy {c} offset {:#x} wanted {want:#x} held {got:#x} memory now {now:#x};                      its page given this submission: {given}",
                    at - bases[c]
                );
                lost.push((submission, c, at - bases[c], want, got));
            }
        }
        if lost.len() > 20 {
            break;
        }
    }
    stop.store(true, Ordering::Relaxed);
    eprintln!(
        "stress: {submissions} submissions, {checked} slots checked, {} writes made",
        counter.load(Ordering::Relaxed)
    );
    for t in threads {
        t.join().unwrap();
    }
    tests::drop_views(section, &[one, two], len);
    // SAFETY: our allocation.
    unsafe {
        windows_sys::Win32::System::Memory::VirtualFree(
            watched as *mut _,
            0,
            windows_sys::Win32::System::Memory::MEM_RELEASE,
        )
    };
    assert!(
        lost.is_empty(),
        "writes lost (submission, copy, offset, wanted at least, the copy held): {lost:#x?}"
    );
}
