//! Bounded, whole-record RAM log. No filesystem or firmware dependency.
pub(crate) struct Records<const N: usize> {
    bytes: [u8; N],
    start: usize,
    len: usize,
}

impl<const N: usize> Records<N> {
    pub(crate) const fn new() -> Self { Self { bytes: [0; N], start: 0, len: 0 } }
    pub(crate) fn clear(&mut self) { self.start = 0; self.len = 0; }
    pub(crate) fn push(&mut self, line: &[u8]) {
        if line.len() + 1 > N { return; }
        while self.len + line.len() + 1 > N {
            loop {
                let end = self.bytes[self.start] == b'\n';
                self.start = (self.start + 1) % N;
                self.len -= 1;
                if end || self.len == 0 { break; }
            }
        }
        for &b in line.iter().chain(core::iter::once(&b'\n')) {
            self.bytes[(self.start + self.len) % N] = b;
            self.len += 1;
        }
    }
    /// A page of complete records, oldest first. skip counts from the newest end.
    pub(crate) fn page(&self, out: &mut [u8], skip: usize, limit: usize) -> (usize, usize) {
        let byte = |i| self.bytes[(self.start + i) % N];
        let total = (0..self.len).filter(|&i| byte(i) == b'\n').count();
        let end = total.saturating_sub(skip);
        let begin = end.saturating_sub(limit);
        let (mut record, mut start, mut used) = (0, 0, 0);
        for i in 0..self.len {
            if byte(i) != b'\n' { continue; }
            if record >= begin && record < end {
                let size = i + 1 - start;
                if used + size > out.len() { break; }
                for j in start..=i { out[used] = byte(j); used += 1; }
            }
            record += 1;
            start = i + 1;
        }
        (used, total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn evicts_complete_utf8_records_and_pages_from_latest() {
        let mut log = Records::<48>::new();
        log.push("打开 /data/说明.txt".as_bytes());
        log.push(b"read offset=0");
        log.push(b"close OK");
        log.push(b"new session");
        let mut out = [0; 48];
        let (n, total) = log.page(&mut out, 0, 2);
        assert_eq!(total, 3);
        assert_eq!(&out[..n], b"close OK\nnew session\n");
        let (n, _) = log.page(&mut out, 2, 2);
        assert_eq!(&out[..n], b"read offset=0\n");
        assert!(core::str::from_utf8(&out[..n]).is_ok());
        log.clear();
        assert_eq!(log.page(&mut out, 0, 2), (0, 0));
    }
    #[test]
    fn bounds_and_wrap_never_return_partial_entries() {
        let mut log = Records::<16>::new();
        for i in 0..100 { log.push(&[b'a' + i % 26; 6]); }
        let mut out = [0; 6];
        assert_eq!(log.page(&mut out, 0, 1), (0, 2));
        let mut out = [0; 16];
        let (n, total) = log.page(&mut out, 0, 20);
        assert_eq!((n, total), (14, 2));
        assert_eq!(out[6], b'\n'); assert_eq!(out[13], b'\n');
        log.push(&[b'x'; 16]);
        assert_eq!(log.page(&mut out, 100, 2), (0, 2));
    }
}
