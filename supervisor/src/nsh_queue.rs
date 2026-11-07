//! Fixed-capacity SPSC byte ring. Capacity is a power of two; counters wrap.
//! No references to the backing array cross producer/consumer operations.
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicUsize, Ordering};

pub(super) struct Spsc<const N: usize> {
    bytes: UnsafeCell<[u8; N]>,
    head: AtomicUsize,
    tail: AtomicUsize,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn full_block_commit_and_usize_counter_wrap_are_lossless() {
        let ring: Spsc<64> = Spsc::new();
        ring.head.store(usize::MAX - 15, Ordering::Relaxed);
        ring.tail.store(usize::MAX - 15, Ordering::Relaxed);
        let initial: Vec<u8> = (0..64).collect();
        unsafe {
            assert!(ring.push_all(&initial));
            assert!(!ring.push_all(b"not partially committed"));
            let mut first = [0u8; 20];
            assert_eq!(ring.pop(&mut first), 20);
            assert_eq!(first.as_slice(), &initial[..20]);
            assert!(ring.push_all(b"abcdef"));
            let mut tail = [0u8; 64];
            assert_eq!(ring.pop(&mut tail), 50);
            assert_eq!(&tail[..44], &initial[20..]);
            assert_eq!(&tail[44..50], b"abcdef");
            assert_eq!(ring.pop(&mut tail), 0);
        }
    }
}
// Every storage operation is unsafe and explicitly requires one role owner.
unsafe impl<const N: usize> Sync for Spsc<N> {}
impl<const N: usize> Spsc<N> {
    pub(super) const fn new() -> Self {
        assert!(N != 0 && N.is_power_of_two());
        Self { bytes: UnsafeCell::new([0; N]), head: AtomicUsize::new(0), tail: AtomicUsize::new(0) }
    }
    /// Caller is the sole producer. The consumer may run concurrently.
    pub(super) unsafe fn push_all(&self, bytes: &[u8]) -> bool {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        if bytes.len() > N - head.wrapping_sub(tail) { return false; }
        self.commit(head, bytes);
        true
    }
    /// Caller is the sole producer. The consumer may run concurrently.
    pub(super) unsafe fn push_some(&self, bytes: &[u8]) -> usize {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        let count = bytes.len().min(N - head.wrapping_sub(tail));
        self.commit(head, &bytes[..count]);
        count
    }
    unsafe fn commit(&self, head: usize, bytes: &[u8]) {
        let start = head & (N - 1);
        let first = bytes.len().min(N - start);
        let dst = self.bytes.get().cast::<u8>();
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), dst.add(start), first);
        core::ptr::copy_nonoverlapping(bytes.as_ptr().add(first), dst, bytes.len() - first);
        self.head.store(head.wrapping_add(bytes.len()), Ordering::Release);
    }
    /// Caller is the sole consumer. The producer may run concurrently.
    pub(super) unsafe fn pop(&self, out: &mut [u8]) -> usize {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        let count = out.len().min(head.wrapping_sub(tail));
        let start = tail & (N - 1);
        let first = count.min(N - start);
        let src = self.bytes.get().cast::<u8>();
        core::ptr::copy_nonoverlapping(src.add(start), out.as_mut_ptr(), first);
        core::ptr::copy_nonoverlapping(src, out.as_mut_ptr().add(first), count - first);
        self.tail.store(tail.wrapping_add(count), Ordering::Release);
        count
    }
    /// Both roles must be quiescent. The backend's final-state Acquire and
    /// next-request Release/Acquire handoff provide this at session restart.
    pub(super) unsafe fn reset_quiescent(&self) {
        self.head.store(0, Ordering::Relaxed);
        self.tail.store(0, Ordering::Relaxed);
    }
}
