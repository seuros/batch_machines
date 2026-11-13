//! Thin native batch policy with caller-owned payload storage and clock.
mod batch;
mod init;

use std::{num::NonZeroU32, time::Duration};

use magnus::{Error, Integer, Ruby, TryConvert, Value};

fn integer(value: Value, message: &'static str) -> Result<(), Error> {
    if Integer::from_value(value).is_some() {
        return Ok(());
    }
    Err(Error::new(
        Ruby::get_with(value).exception_type_error(),
        message,
    ))
}

/// An instant or a span: Integer nanoseconds in `0..2**64`.
#[derive(Clone, Copy)]
struct Nanos(Duration);

impl TryConvert for Nanos {
    fn try_convert(value: Value) -> Result<Self, Error> {
        integer(value, "time must be an Integer number of nanoseconds")?;
        u64::try_convert(value).map(|nanos| Self(Duration::from_nanos(nanos)))
    }
}

/// A capacity or a cost: an Integer in `1..2**32`.
#[derive(Clone, Copy)]
struct Cost(NonZeroU32);

impl TryConvert for Cost {
    fn try_convert(value: Value) -> Result<Self, Error> {
        integer(value, "capacity and cost must be Integers")?;
        NonZeroU32::new(u32::try_convert(value)?)
            .map(Self)
            .ok_or_else(|| {
                Error::new(
                    Ruby::get_with(value).exception_range_error(),
                    "capacity and cost must be in 1..2**32",
                )
            })
    }
}
