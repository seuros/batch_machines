use core::fmt;

/// Why a call was not honoured. A refused call changes nothing: no item is
/// counted, no cost is held, and the clock it carried is not observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// `now` is earlier than the latest instant the batch observed.
    TimeRegression,
    /// A value the call had to compute is out of range: `now + max_wait` does
    /// not fit a `Duration` (a push that would start a batch), or a term of
    /// the PID calculation is not finite (`adapt`).
    Overflow,
    /// The item costs more than the capacity still unspent. Flush first; an
    /// item costing more than the whole capacity is never admitted.
    Full,
    /// The PID error (`adapt`) or the output (`retarget`) is NaN or infinite.
    NotFinite,
    /// The elapsed time handed to `adapt` is zero.
    InvalidDuration,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::TimeRegression => "now is earlier than the latest observed instant",
            Self::Overflow => "a computed deadline or controller term is out of range",
            Self::Full => "the item costs more than the capacity still unspent",
            Self::NotFinite => "the error or output is NaN or infinite",
            Self::InvalidDuration => "the elapsed time is zero",
        })
    }
}

impl core::error::Error for Refusal {}
