//! Thin native batch policy with caller-owned payload storage and clock.
mod batch;
mod init;

use std::time::Duration;

use magnus::{Error, Integer, Ruby, TryConvert, Value};

#[derive(Clone, Copy)]
struct Count(u64);

impl TryConvert for Count {
    fn try_convert(value: Value) -> Result<Self, Error> {
        if Integer::from_value(value).is_none() {
            return Err(Error::new(
                Ruby::get_with(value).exception_type_error(),
                "count/time must be an unsigned 64-bit Integer (time in nanoseconds)",
            ));
        }
        u64::try_convert(value).map(Self)
    }
}

impl Count {
    const fn duration(self) -> Duration {
        Duration::from_nanos(self.0)
    }
}
