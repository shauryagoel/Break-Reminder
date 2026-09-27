use std::time::{Duration, Instant};

#[derive(Debug, PartialEq, Eq)]
pub enum Tick {
    LaunchOverlay(u64),
}

#[derive(Clone, Copy)]
pub enum Completion {
    Elapsed,
    Skip,
    Postpone(Duration),
    Failed,
    Closed,
}

enum State {
    Waiting(Instant),
    Paused(Duration),
    Launching { id: u64, display: Duration },
    Showing { id: u64, deadline: Instant },
}

pub struct Timer {
    interval: Duration,
    display: Duration,
    state: State,
    next_id: u64,
}

impl Timer {
    pub fn new(interval: Duration, display: Duration, now: Instant) -> Self {
        Self {
            interval,
            display,
            state: State::Waiting(now + interval),
            next_id: 0,
        }
    }

    pub fn deadline(&self) -> Option<Instant> {
        match self.state {
            State::Waiting(deadline) => Some(deadline),
            State::Paused(_) | State::Launching { .. } | State::Showing { .. } => None,
        }
    }

    pub fn is_paused(&self) -> bool {
        matches!(self.state, State::Paused(_))
    }

    pub fn pause(&mut self, now: Instant) -> bool {
        if let State::Waiting(deadline) = self.state {
            self.state = State::Paused(deadline.saturating_duration_since(now));
            true
        } else {
            false
        }
    }

    pub fn resume(&mut self, now: Instant) -> bool {
        if let State::Paused(remaining) = self.state {
            self.state = State::Waiting(now + remaining);
            true
        } else {
            false
        }
    }

    pub fn reload(&mut self, interval: Duration, display: Duration, now: Instant) {
        match &mut self.state {
            State::Waiting(deadline) => *deadline = now + interval,
            State::Paused(remaining) => *remaining = interval,
            State::Launching { .. } | State::Showing { .. } => {}
        }
        self.interval = interval;
        self.display = display;
    }

    pub fn tick(&mut self, now: Instant) -> Option<Tick> {
        match self.state {
            State::Waiting(deadline) if now >= deadline => {
                self.next_id = self.next_id.checked_add(1).expect("overlay ID exhausted");
                self.state = State::Launching {
                    id: self.next_id,
                    display: self.display,
                };
                Some(Tick::LaunchOverlay(self.next_id))
            }
            _ => None,
        }
    }

    pub fn visible(&mut self, id: u64, now: Instant) -> bool {
        match self.state {
            State::Launching {
                id: current,
                display,
            } if current == id => {
                self.state = State::Showing {
                    id,
                    deadline: now + display,
                };
                true
            }
            _ => false,
        }
    }

    pub fn display_remaining(&self, now: Instant) -> Option<Duration> {
        match self.state {
            State::Showing { deadline, .. } => Some(deadline.saturating_duration_since(now)),
            _ => None,
        }
    }

    pub fn complete(&mut self, id: u64, completion: Completion, now: Instant) -> bool {
        let valid = match completion {
            Completion::Elapsed => {
                matches!(self.state, State::Showing { id: current, deadline } if current == id && now >= deadline)
            }
            _ => {
                matches!(self.state, State::Launching { id: current, .. } | State::Showing { id: current, .. } if current == id)
            }
        };
        if !valid {
            return false;
        }
        let delay = match completion {
            Completion::Postpone(delay) => delay,
            Completion::Elapsed | Completion::Skip | Completion::Failed | Completion::Closed => {
                self.interval
            }
        };
        self.state = State::Waiting(now + delay);
        true
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{Completion, Tick, Timer};

    fn seconds(value: u64) -> Duration {
        Duration::from_secs(value)
    }

    #[test]
    fn startup_is_due_once_at_or_after_the_interval() {
        let start = Instant::now();
        let mut timer = Timer::new(seconds(60), seconds(30), start);
        assert_eq!(timer.deadline(), Some(start + seconds(60)));
        assert_eq!(timer.tick(start + seconds(59)), None);
        assert_eq!(
            timer.tick(start + seconds(60)),
            Some(Tick::LaunchOverlay(1))
        );
        assert_eq!(timer.tick(start + seconds(3_600)), None);
        assert_eq!(timer.deadline(), None);

        let mut late = Timer::new(seconds(60), seconds(30), start);
        assert_eq!(
            late.tick(start + seconds(3_600)),
            Some(Tick::LaunchOverlay(1))
        );
        assert_eq!(late.tick(start + seconds(7_200)), None);
    }

    #[test]
    fn display_countdown_starts_only_when_visible_and_clamps_at_zero() {
        let start = Instant::now();
        let mut timer = Timer::new(seconds(60), seconds(30), start);
        assert!(!timer.visible(1, start));
        assert_eq!(
            timer.tick(start + seconds(60)),
            Some(Tick::LaunchOverlay(1))
        );
        assert_eq!(timer.display_remaining(start + seconds(65)), None);
        assert!(timer.visible(1, start + seconds(65)));
        assert!(!timer.visible(1, start + seconds(66)));
        assert_eq!(timer.deadline(), None);
        assert_eq!(
            timer.display_remaining(start + seconds(94)),
            Some(seconds(1))
        );
        assert_eq!(
            timer.display_remaining(start + seconds(100)),
            Some(Duration::ZERO)
        );
        assert_eq!(timer.tick(start + seconds(94)), None);
        assert_eq!(timer.tick(start + seconds(95)), None);
        assert_eq!(timer.tick(start + seconds(100)), None);
        assert_eq!(
            timer.display_remaining(start + seconds(100)),
            Some(Duration::ZERO)
        );
        assert!(timer.complete(1, Completion::Elapsed, start + seconds(100)));
        assert_eq!(timer.deadline(), Some(start + seconds(160)));
        assert!(!timer.complete(1, Completion::Closed, start + seconds(101)));
        assert_eq!(timer.deadline(), Some(start + seconds(160)));
        assert_eq!(timer.display_remaining(start + seconds(101)), None);
    }

    #[test]
    fn elapsed_before_visibility_or_display_deadline_is_ignored() {
        let start = Instant::now();
        let mut timer = Timer::new(seconds(60), seconds(30), start);
        assert_eq!(
            timer.tick(start + seconds(60)),
            Some(Tick::LaunchOverlay(1))
        );
        assert!(!timer.complete(1, Completion::Elapsed, start + seconds(90)));
        assert_eq!(timer.deadline(), None);
        assert!(timer.visible(1, start + seconds(100)));
        assert!(!timer.complete(1, Completion::Elapsed, start + seconds(129)));
        assert_eq!(
            timer.display_remaining(start + seconds(129)),
            Some(seconds(1))
        );
        assert!(timer.complete(1, Completion::Elapsed, start + seconds(130)));
        assert_eq!(timer.deadline(), Some(start + seconds(190)));
    }

    #[test]
    fn elapsed_skip_failure_and_close_start_a_full_interval_once() {
        let start = Instant::now();
        for (completion, visible, finish_second) in [
            (Completion::Elapsed, true, 91),
            (Completion::Skip, true, 65),
            (Completion::Failed, false, 65),
            (Completion::Closed, true, 65),
        ] {
            let mut timer = Timer::new(seconds(60), seconds(30), start);
            assert_eq!(
                timer.tick(start + seconds(60)),
                Some(Tick::LaunchOverlay(1))
            );
            if visible {
                assert!(timer.visible(1, start + seconds(61)));
            }
            assert!(timer.complete(1, completion, start + seconds(finish_second)));
            assert_eq!(timer.deadline(), Some(start + seconds(finish_second + 60)));
            assert!(!timer.complete(
                1,
                Completion::Postpone(seconds(600)),
                start + seconds(finish_second + 1)
            ));
            assert_eq!(timer.deadline(), Some(start + seconds(finish_second + 60)));
        }
    }

    #[test]
    fn postpone_uses_the_selected_relative_delay_and_wins_over_later_exit() {
        let start = Instant::now();
        let mut timer = Timer::new(seconds(60), seconds(30), start);
        assert!(!timer.complete(1, Completion::Skip, start));
        assert_eq!(
            timer.tick(start + seconds(60)),
            Some(Tick::LaunchOverlay(1))
        );
        assert!(timer.visible(1, start + seconds(63)));
        assert_eq!(timer.tick(start + seconds(100)), None);
        assert!(timer.complete(1, Completion::Postpone(seconds(600)), start + seconds(100)));
        assert_eq!(timer.deadline(), Some(start + seconds(700)));
        assert!(!timer.complete(1, Completion::Elapsed, start + seconds(101)));
        assert!(!timer.complete(1, Completion::Closed, start + seconds(102)));
        assert_eq!(timer.deadline(), Some(start + seconds(700)));
    }

    #[test]
    fn late_signals_from_an_old_overlay_cannot_change_the_next_overlay() {
        let start = Instant::now();
        let mut timer = Timer::new(seconds(60), seconds(30), start);
        let Some(Tick::LaunchOverlay(first)) = timer.tick(start + seconds(60)) else {
            panic!("first overlay did not launch");
        };
        assert!(timer.complete(
            first,
            Completion::Postpone(seconds(60)),
            start + seconds(61)
        ));
        let Some(Tick::LaunchOverlay(second)) = timer.tick(start + seconds(121)) else {
            panic!("second overlay did not launch");
        };
        assert_ne!(first, second);
        assert!(!timer.visible(first, start + seconds(122)));
        assert!(!timer.complete(first, Completion::Closed, start + seconds(122)));
        assert_eq!(timer.deadline(), None);
        assert!(timer.visible(second, start + seconds(123)));
        assert!(timer.complete(second, Completion::Skip, start + seconds(124)));
        assert_eq!(timer.deadline(), Some(start + seconds(184)));
    }

    #[test]
    fn pause_freezes_remaining_time_until_resume() {
        let start = Instant::now();
        let mut timer = Timer::new(seconds(60), seconds(30), start);
        assert!(!timer.is_paused());
        assert!(timer.pause(start + seconds(20)));
        assert!(timer.is_paused());
        assert_eq!(timer.deadline(), None);
        assert!(!timer.pause(start + seconds(30)));
        assert_eq!(timer.tick(start + seconds(200)), None);
        assert!(timer.resume(start + seconds(200)));
        assert!(!timer.is_paused());
        assert!(!timer.resume(start + seconds(201)));
        assert_eq!(timer.deadline(), Some(start + seconds(240)));
        assert_eq!(timer.tick(start + seconds(239)), None);
        assert_eq!(
            timer.tick(start + seconds(240)),
            Some(Tick::LaunchOverlay(1))
        );
        assert!(!timer.pause(start + seconds(241)));
        assert!(!timer.resume(start + seconds(241)));
        assert!(timer.visible(1, start + seconds(242)));
        assert!(!timer.pause(start + seconds(243)));
        assert!(!timer.resume(start + seconds(243)));
    }

    #[test]
    fn reload_resets_waiting_and_paused_intervals() {
        let start = Instant::now();
        let mut timer = Timer::new(seconds(60), seconds(30), start);
        timer.reload(seconds(90), seconds(45), start + seconds(20));
        assert_eq!(timer.deadline(), Some(start + seconds(110)));
        assert!(timer.pause(start + seconds(30)));
        timer.reload(seconds(120), seconds(50), start + seconds(40));
        assert!(timer.is_paused());
        assert_eq!(timer.deadline(), None);
        assert!(timer.resume(start + seconds(100)));
        assert_eq!(timer.deadline(), Some(start + seconds(220)));
        assert_eq!(
            timer.tick(start + seconds(220)),
            Some(Tick::LaunchOverlay(1))
        );
        assert!(timer.visible(1, start + seconds(221)));
        assert_eq!(timer.deadline(), None);
        assert_eq!(
            timer.display_remaining(start + seconds(221)),
            Some(seconds(50))
        );
    }

    #[test]
    fn reload_during_launch_keeps_current_display_and_updates_next_interval() {
        let start = Instant::now();
        let mut timer = Timer::new(seconds(60), seconds(30), start);
        assert_eq!(
            timer.tick(start + seconds(60)),
            Some(Tick::LaunchOverlay(1))
        );
        timer.reload(seconds(120), seconds(45), start + seconds(61));
        assert_eq!(timer.deadline(), None);
        assert!(timer.visible(1, start + seconds(63)));
        assert_eq!(
            timer.display_remaining(start + seconds(63)),
            Some(seconds(30))
        );
        assert_eq!(timer.tick(start + seconds(93)), None);
        assert!(timer.complete(1, Completion::Elapsed, start + seconds(93)));
        assert_eq!(timer.deadline(), Some(start + seconds(213)));
    }

    #[test]
    fn reload_during_display_keeps_countdown_and_updates_next_overlay() {
        let start = Instant::now();
        let mut timer = Timer::new(seconds(60), seconds(30), start);
        assert_eq!(
            timer.tick(start + seconds(60)),
            Some(Tick::LaunchOverlay(1))
        );
        assert!(timer.visible(1, start + seconds(61)));
        assert_eq!(
            timer.display_remaining(start + seconds(61)),
            Some(seconds(30))
        );
        timer.reload(seconds(120), seconds(45), start + seconds(65));
        assert_eq!(
            timer.display_remaining(start + seconds(65)),
            Some(seconds(26))
        );
        assert_eq!(timer.tick(start + seconds(91)), None);
        assert!(timer.complete(1, Completion::Elapsed, start + seconds(91)));
        assert_eq!(timer.deadline(), Some(start + seconds(211)));
        assert_eq!(
            timer.tick(start + seconds(211)),
            Some(Tick::LaunchOverlay(2))
        );
        assert!(timer.visible(2, start + seconds(212)));
        assert_eq!(
            timer.display_remaining(start + seconds(212)),
            Some(seconds(45))
        );
    }
}
