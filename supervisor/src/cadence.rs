//! Wall-clock cadence independent of the shared maintenance timer frequency.
pub(crate) struct Cadence { last: Option<u32> }
impl Cadence {
    pub(crate) const fn new() -> Self { Self { last: None } }
    pub(crate) fn due(&self, now: u32, interval: u32) -> bool {
        self.last.map_or(true, |last| now.wrapping_sub(last) >= interval)
    }
    pub(crate) fn take(&mut self, now: u32, interval: u32) -> bool {
        if !self.due(now, interval) { return false; }
        self.last = Some(now); true
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn frequency_changes_do_not_accelerate_work() {
        let mut clock=Cadence::new();
        assert!(clock.take(0,1000));
        for now in (50..1000).step_by(50) { assert!(!clock.take(now,1000)); }
        assert!(clock.take(1000,1000));
        assert!(!clock.take(1200,1000));
        assert!(clock.take(5200,1000)); // one execution, no catch-up burst
        assert!(!clock.take(5200,1000));
    }
    #[test] fn timer_wrap_and_new_interval() {
        let mut clock=Cadence::new();
        assert!(clock.take(u32::MAX-99,1000));
        assert!(!clock.take(899,1000));
        assert!(clock.take(900,1000));
        assert!(!clock.due(949,50));
        assert!(clock.due(950,50));
    }
}
