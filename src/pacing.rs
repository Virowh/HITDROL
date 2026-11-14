use std::time::{Duration, Instant};

/// One gate for *all* presentation requests, including pointer-driven redraws.
pub struct FramePacer {
    pub limit: u32,
    last: Option<Instant>,
    dirty: bool,
    wake: Option<Instant>,
    pub visible: bool,
}
impl FramePacer {
    pub fn new(limit: u32) -> Self {
        Self {
            limit: limit.clamp(1, 240),
            last: None,
            dirty: true,
            wake: None,
            visible: true,
        }
    }
    pub fn request(&mut self) {
        self.dirty = true;
    }
    pub fn request_after(&mut self, now: Instant, delay: Duration) {
        if let Some(at) = now.checked_add(delay) {
            self.wake = Some(self.wake.map_or(at, |old| old.min(at)));
        }
    }
    pub fn deadline(&self, now: Instant) -> Option<Instant> {
        if !self.visible {
            return None;
        }
        let wanted = if self.dirty { now } else { self.wake? };
        let gate = self.last.map_or(now, |last| {
            last + Duration::from_secs_f64(1.0 / self.limit.max(1) as f64)
        });
        Some(wanted.max(gate))
    }
    pub fn ready(&self, now: Instant) -> bool {
        self.deadline(now).is_some_and(|d| d <= now)
    }
    pub fn presented(&mut self, now: Instant) {
        self.last = Some(now);
        self.dirty = false;
        if self.wake.is_some_and(|d| d <= now) {
            self.wake = None;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pointer_storm_is_capped() {
        let t = Instant::now();
        let mut p = FramePacer::new(60);
        assert!(p.ready(t));
        p.presented(t);
        for n in 1..16 {
            p.request();
            assert!(!p.ready(t + Duration::from_millis(n)));
        }
        assert!(p.ready(t + Duration::from_millis(17)));
    }
    #[test]
    fn idle_and_hidden_do_not_render() {
        let t = Instant::now();
        let mut p = FramePacer::new(60);
        p.presented(t);
        assert!(p.deadline(t).is_none());
        p.request();
        p.visible = false;
        assert!(p.deadline(t + Duration::from_secs(1)).is_none());
    }
    #[test]
    fn delayed_repaint_survives_gate() {
        let t = Instant::now();
        let mut p = FramePacer::new(30);
        p.presented(t);
        p.request_after(t, Duration::from_millis(10));
        assert!(!p.ready(t + Duration::from_millis(11)));
        assert!(p.ready(t + Duration::from_millis(34)));
    }
}
