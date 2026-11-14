#![no_std]
#![doc = include_str!("../README.md")]

mod refusal;

use core::{num::NonZeroU32, time::Duration};

use flow_machines::CreditWindow;

pub use feedback_machines::{Gains, Limits, Pid};
pub use refusal::Refusal;

/// Why pending work should flush.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlushReason {
    /// Target cost reached.
    Size,
    /// Oldest item's wait budget expired. Wins over `Size` when both hold.
    Deadline,
}

/// What an honoured push did. The item is counted either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    /// The item was buffered and nothing is due.
    Buffered,
    /// The item was buffered and a flush is due.
    Flush(FlushReason),
}

/// Bounded batch policy, with caller-owned payloads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Batch {
    credits: CreditWindow,
    capacity: NonZeroU32,
    items: u32,
    target: NonZeroU32,
    max_wait: Duration,
    deadline: Option<Duration>,
    observed: Duration,
}

impl Batch {
    /// Start empty, with a capacity-sized target. `None` for a zero
    /// `max_wait`: the first item would be due on arrival.
    #[must_use]
    pub const fn new(capacity: NonZeroU32, max_wait: Duration) -> Option<Self> {
        if max_wait.is_zero() {
            return None;
        }
        Some(Self {
            credits: CreditWindow::new(capacity),
            capacity,
            items: 0,
            target: capacity,
            max_wait,
            deadline: None,
            observed: Duration::ZERO,
        })
    }

    /// Account one item of cost 1; the caller stores its payload only when
    /// the push is honoured. Exactly `push_weighted(now, 1)`.
    ///
    /// # Errors
    ///
    /// As [`Self::push_weighted`].
    pub fn push(&mut self, now: Duration) -> Result<Admission, Refusal> {
        self.push_weighted(now, NonZeroU32::MIN)
    }

    /// Account one item costing `cost` capacity units; the caller stores its
    /// payload only when the push is honoured.
    ///
    /// # Errors
    ///
    /// [`Refusal::TimeRegression`] when `now` is earlier than the latest
    /// observed instant, [`Refusal::Overflow`] when the item would start a
    /// batch and `now + max_wait` does not fit a `Duration`, [`Refusal::Full`]
    /// when `cost` exceeds the unspent capacity. Nothing changed: `now` is
    /// not observed.
    pub fn push_weighted(&mut self, now: Duration, cost: NonZeroU32) -> Result<Admission, Refusal> {
        self.check(now)?;
        let deadline = match self.deadline {
            Some(deadline) => deadline,
            None => now.checked_add(self.max_wait).ok_or(Refusal::Overflow)?,
        };
        // Ask the window first, then act: an acquisition within `available()`
        // cannot be refused, so no flow refusal is mapped onto `Full`.
        if cost.get() > self.credits.available() {
            return Err(Refusal::Full);
        }
        match self.credits.try_acquire(cost) {
            Ok(()) => {}
            Err(
                flow_machines::Refusal::Insufficient { .. }
                | flow_machines::Refusal::ExceedsInFlight { .. },
            ) => {
                debug_assert!(false, "the window refused a cost within available()");
                return Err(Refusal::Full);
            }
        }
        self.observed = now;
        self.items += 1;
        self.deadline = Some(deadline);
        Ok(self
            .reason(now)
            .map_or(Admission::Buffered, Admission::Flush))
    }

    /// Recommend a flush without changing pending work: `Ok(None)` is the
    /// honoured answer "nothing is due at this instant". Every honoured poll
    /// observes `now`, `Ok(None)` included: the answer depends on it.
    ///
    /// # Errors
    ///
    /// [`Refusal::TimeRegression`] when `now` is earlier than the latest
    /// observed instant; nothing changed.
    pub fn poll(&mut self, now: Duration) -> Result<Option<FlushReason>, Refusal> {
        self.check(now)?;
        self.observed = now;
        Ok(self.reason(now))
    }

    /// Acknowledge a full flush or cancellation: every pending item leaves,
    /// the deadline is cleared, and the old item count is the answer.
    ///
    /// A flush that empties the batch observes `now`. Flushing an empty batch
    /// is an honoured repeat: it answers zero whatever the time, so it
    /// changes nothing and observes nothing.
    ///
    /// # Errors
    ///
    /// [`Refusal::TimeRegression`] when `now` is earlier than the latest
    /// observed instant; nothing changed.
    pub fn flush(&mut self, now: Duration) -> Result<u32, Refusal> {
        self.check(now)?;
        if self.items == 0 {
            return Ok(0);
        }
        self.observed = now;
        self.credits = CreditWindow::new(self.capacity);
        self.deadline = None;
        Ok(core::mem::take(&mut self.items))
    }

    /// Step the caller's controller once and aim the target at its output:
    /// exactly `pid.step(error, elapsed)` then [`Self::retarget`]. The answer
    /// is the new target.
    ///
    /// It is `adapt`, not `adapt_from`: the controller is stepped, not read,
    /// so its integral and derivative history move with every honoured call.
    ///
    /// # Errors
    ///
    /// [`Refusal::NotFinite`] for a NaN or infinite `error`,
    /// [`Refusal::InvalidDuration`] for a zero `elapsed`, [`Refusal::Overflow`]
    /// when a term of the PID calculation is not finite. Nothing changed, in
    /// the batch or in the controller.
    pub fn adapt(&mut self, pid: &mut Pid, error: f64, elapsed: Duration) -> Result<u32, Refusal> {
        match pid.step(error, elapsed) {
            // A PID output lies within its finite limits.
            Ok(output) => Ok(self.aim(output)),
            Err(feedback_machines::Refusal::NotFinite) => Err(Refusal::NotFinite),
            Err(feedback_machines::Refusal::InvalidDuration) => Err(Refusal::InvalidDuration),
            Err(feedback_machines::Refusal::Overflow) => Err(Refusal::Overflow),
        }
    }

    /// Aim the target at `output`, a cost: truncated toward zero and clamped
    /// to `1..=capacity`. The answer is the new target. Pending work and its
    /// deadline are untouched; setting the target it already has changes
    /// nothing.
    ///
    /// # Errors
    ///
    /// [`Refusal::NotFinite`] for a NaN or infinite `output`; nothing changed.
    pub fn retarget(&mut self, output: f64) -> Result<u32, Refusal> {
        if output.is_finite() {
            Ok(self.aim(output))
        } else {
            Err(Refusal::NotFinite)
        }
    }

    /// Hard admission limit, in cost units.
    #[must_use]
    pub const fn capacity(&self) -> u32 {
        self.capacity.get()
    }

    /// Longest the oldest pending item waits before `Deadline` is due.
    #[must_use]
    pub const fn max_wait(&self) -> Duration {
        self.max_wait
    }

    /// Number of accepted items not yet acknowledged by `flush`.
    #[must_use]
    pub const fn pending(&self) -> u32 {
        self.items
    }

    /// Capacity units held by pending items.
    #[must_use]
    pub const fn pending_cost(&self) -> u32 {
        self.credits.in_flight()
    }

    /// Current target cost; capacity remains the hard admission limit.
    #[must_use]
    pub const fn target(&self) -> u32 {
        self.target.get()
    }

    /// Absolute time at which `poll` first reports `Deadline`; `None` while
    /// nothing is pending.
    #[must_use]
    pub const fn deadline(&self) -> Option<Duration> {
        self.deadline
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn aim(&mut self, output: f64) -> u32 {
        // Clamped to `1.0..=capacity`, so the cast is exact and at least one:
        // the target is one plus the rest, and no zero arm exists.
        let cost = output.clamp(1.0, f64::from(self.capacity.get())) as u32;
        self.target = NonZeroU32::MIN.saturating_add(cost.saturating_sub(1));
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

    /// Refuse a regressed clock without recording anything.
    const fn check(&self, now: Duration) -> Result<(), Refusal> {
        if now.as_nanos() < self.observed.as_nanos() {
            return Err(Refusal::TimeRegression);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
