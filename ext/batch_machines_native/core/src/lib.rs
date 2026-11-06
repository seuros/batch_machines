#![no_std]
#![doc = include_str!("../README.md")]
use core::{num::NonZeroU32, time::Duration};
use feedback_machines::Pid;
use flow_machines::CreditWindow;

/// Why pending work should flush.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FlushReason {
    /// Target size reached.
    Size,
    /// Oldest item's wait budget expired.
    Deadline,
}

/// Admission decision for one item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Admission {
    /// Item was buffered.
    Buffered,
    /// Item was buffered and a flush is due.
    Flush(FlushReason),
    /// Capacity exhausted; item was not accepted.
    Full,
    /// Invalid time or deadline overflow; item was not accepted.
    Rejected,
}

/// Bounded batch policy, with caller-owned payloads.
#[derive(Debug)]
pub struct Batch {
    credits: CreditWindow,
    target: NonZeroU32,
    max_wait: Duration,
    deadline: Option<Duration>,
    observed: Duration,
}

impl Batch {
    /// Start empty, with a positive maximum wait and capacity-sized target.
    #[must_use]
    pub const fn new(capacity: NonZeroU32, max_wait: Duration) -> Option<Self> {
        if max_wait.is_zero() {
            return None;
        }
        Some(Self {
            credits: CreditWindow::new(capacity),
            target: capacity,
            max_wait,
            deadline: None,
            observed: Duration::ZERO,
        })
    }

    /// Account one item; the caller stores its payload only on acceptance.
    #[must_use]
    pub fn push(&mut self, now: Duration) -> Admission {
        if !self.observe(now) {
            return Admission::Rejected;
        }
        let Some(deadline) = self.deadline.or_else(|| now.checked_add(self.max_wait)) else {
            return Admission::Rejected;
        };
        if !self.credits.try_acquire(NonZeroU32::MIN) {
            return Admission::Full;
        }
        self.deadline = Some(deadline);
        self.reason(now)
            .map_or(Admission::Buffered, Admission::Flush)
    }

    /// Recommend a flush without changing pending work.
    #[must_use]
    pub fn poll(&mut self, now: Duration) -> Option<FlushReason> {
        if !self.observe(now) {
            return None;
        }
        self.reason(now)
    }

    /// Acknowledge a full flush/cancellation and return the old item count.
    ///
    /// # Panics
    /// Panics if internal capacity accounting is inconsistent.
    pub fn flush(&mut self, now: Duration) -> Option<u32> {
        if !self.observe(now) {
            return None;
        }
        let count = self.credits.in_flight();
        if let Some(count) = NonZeroU32::new(count) {
            assert!(self.credits.release(count));
        }
        self.deadline = None;
        Some(count)
    }

    /// Interpret finite PID output as a target item count, clamped to capacity.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub fn adapt(&mut self, pid: &mut Pid, error: f64, elapsed: Duration) -> Option<u32> {
        let output = pid.step(error, elapsed)?;
        let count = output.clamp(1.0, f64::from(self.credits.capacity())) as u32;
        self.target = NonZeroU32::new(count)?;
        Some(count)
    }

    /// Number of accepted items not yet acknowledged by `flush`.
    #[must_use]
    pub const fn pending(&self) -> u32 {
        self.credits.in_flight()
    }

    /// Current target size; capacity remains the hard admission limit.
    #[must_use]
    pub const fn target(&self) -> u32 {
        self.target.get()
    }

    fn reason(&self, now: Duration) -> Option<FlushReason> {
        if self.deadline.is_some_and(|deadline| now >= deadline) {
            Some(FlushReason::Deadline)
        } else if self.credits.in_flight() >= self.target.get() {
            Some(FlushReason::Size)
        } else {
            None
        }
    }

    fn observe(&mut self, now: Duration) -> bool {
        if now < self.observed {
            return false;
        }
        self.observed = now;
        true
    }
}

#[cfg(test)]
mod tests;
