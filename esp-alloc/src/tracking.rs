//! Allocation tracking via [`defmt`] for offline heap analysis.
//!
//! # Wire format
//!
//! Records are emitted as [`defmt::trace!()`] raw byte slices tagged with the prefix `"hp:"`.
//! The binary payload layout is defined as follows.
//!
//! ```text
//! [tag: u8][ptr: u32 LE][size: u32 LE][nframes: u8][pc_0..n: u32 LE each]
//! ```
//!
//! Tags: `0x41` ('A') = alloc, `0x44` ('D') = dealloc.
//!
//! # Re-entrance guard
//!
//! The [`defmt`] transport used by esp-alloc does not allocate. The [`AtomicBool`] guard is a
//! safety net in case a future transport change introduces allocations during emission.

use core::sync::atomic::{
    AtomicBool,
    Ordering,
};

// Number of frame-pointer frames captured per event.
const MAX_FRAMES: usize = 8;

const TAG_ALLOC: u8 = b'A';
const TAG_DEALLOC: u8 = b'D';

// Prevents recursive emission if the transport layer ever allocates.
pub(crate) static IN_TRACKER: AtomicBool = AtomicBool::new(false);

/// Walk the RISC-V frame-pointer chain and write return addresses into `out`.
///
/// Returns the number of frames written. Requires the binary to be built
/// with `-C force-frame-pointers`.
#[cfg(target_arch = "riscv32")]
#[inline(never)]
fn capture_backtrace(out: &mut [u32; MAX_FRAMES]) -> usize {
    let mut fp: u32;

    // SAFETY: reading s0 is always safe because it is a callee-saved register that the compiler
    // keeps valid across function calls.
    unsafe {
        core::arch::asm!(
            "mv {fp}, s0",
            fp = out(reg) fp,
            options(nomem, nostack, preserves_flags),
        );
    }

    let mut n = 0;
    while n < MAX_FRAMES && fp != 0 && fp & 3 == 0 {
        // SAFETY: fp came from s0 which the compiler maintains as a valid aligned stack address.
        // Alignment and null are checked above.
        let ra = unsafe { (fp.wrapping_sub(4) as *const u32).read_unaligned() };
        let next_fp = unsafe { (fp.wrapping_sub(8) as *const u32).read_unaligned() };

        if ra <= 1 {
            break;
        }
        out[n] = ra;
        n += 1;
        fp = next_fp;
    }

    n
}

/// No-op stub for non-RISC-V targets.
#[cfg(not(target_arch = "riscv32"))]
#[inline(always)]
fn capture_backtrace(_out: &mut [u32; MAX_FRAMES]) -> usize {
    0
}

/// Emits one allocation record over defmt.
#[inline(never)]
pub(crate) fn emit(tag: u8, ptr: u32, size: u32, frames: &[u32]) {
    let mut buf = [0u8; 42];
    let nframes = frames.len() as u8;
    buf[0] = tag;
    buf[1..5].copy_from_slice(&ptr.to_le_bytes());
    buf[5..9].copy_from_slice(&size.to_le_bytes());
    buf[9] = nframes;
    for (i, &pc) in frames.iter().enumerate() {
        let off = 10 + i * 4;
        buf[off..off + 4].copy_from_slice(&pc.to_le_bytes());
    }
    defmt::trace!("hp:{=[u8]}", &buf[..10 + nframes as usize * 4]);
}

/// Records an allocation event if the re-entrance guard permits.
#[inline(always)]
pub(crate) fn track_alloc(ptr: *mut u8, size: usize) {
    if ptr.is_null() {
        return;
    }
    if IN_TRACKER
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_ok()
    {
        let mut frames = [0u32; MAX_FRAMES];
        let n = capture_backtrace(&mut frames);
        emit(TAG_ALLOC, ptr as u32, size as u32, &frames[..n]);
        IN_TRACKER.store(false, Ordering::Release);
    }
}

/// Records a de-allocation event if the re-entrance guard permits.
#[inline(always)]
pub(crate) fn track_dealloc(ptr: *mut u8, size: usize) {
    if ptr.is_null() {
        return;
    }
    if IN_TRACKER
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_ok()
    {
        let mut frames = [0u32; MAX_FRAMES];
        let n = capture_backtrace(&mut frames);
        emit(TAG_DEALLOC, ptr as u32, size as u32, &frames[..n]);
        IN_TRACKER.store(false, Ordering::Release);
    }
}
