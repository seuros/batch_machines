use std::{cell::RefCell, num::NonZeroU32};

use batch_machines::{Admission, Batch, FlushReason};
use feedback_machines::{Gains, Limits, Pid};
use magnus::{Error, RModule, Ruby, function, method, prelude::*, typed_data::Obj};

use crate::Count;

struct State {
    batch: Batch,
    pid: Pid,
}

#[magnus::wrap(class = "BatchMachines::Batch", free_immediately, size)]
struct Wrapper {
    state: RefCell<State>,
    capacity: u32,
}

impl Wrapper {
    fn new(ruby: &Ruby, capacity: Count, wait: Count) -> Result<Self, Error> {
        let capacity = u32::try_from(capacity.0)
            .ok()
            .and_then(NonZeroU32::new)
            .ok_or_else(|| {
                Error::new(ruby.exception_arg_error(), "capacity must be in 1..2**32")
            })?;
        let batch = Batch::new(capacity, wait.duration())
            .ok_or_else(|| Error::new(ruby.exception_arg_error(), "max wait must be positive"))?;
        let pid = Self::pid(ruby, capacity.get(), 1.0, 0.0, 0.0)?;
        Ok(Self {
            state: RefCell::new(State { batch, pid }),
            capacity: capacity.get(),
        })
    }

    fn pid(ruby: &Ruby, capacity: u32, p: f64, i: f64, d: f64) -> Result<Pid, Error> {
        let gains = Gains::new(p, i, d).ok_or_else(|| {
            Error::new(
                ruby.exception_arg_error(),
                "gains must be finite and nonnegative",
            )
        })?;
        let limits = Limits::new(0.0, f64::from(capacity))
            .ok_or_else(|| Error::new(ruby.exception_arg_error(), "invalid PID output limits"))?;
        Ok(Pid::new(gains, limits))
    }

    fn configure_pid(ruby: &Ruby, this: Obj<Self>, p: f64, i: f64, d: f64) -> Result<(), Error> {
        this.check_frozen()?;
        let pid = Self::pid(ruby, this.capacity, p, i, d)?;
        this.state.borrow_mut().pid = pid;
        Ok(())
    }

    fn push(this: Obj<Self>, now: Count) -> Result<&'static str, Error> {
        this.check_frozen()?;
        let result = this.state.borrow_mut().batch.push(now.duration());
        Ok(match result {
            Admission::Buffered => "buffered",
            Admission::Flush(FlushReason::Size) => "size",
            Admission::Flush(_) => "deadline",
            Admission::Full => "full",
            _ => "rejected",
        })
    }

    fn poll(this: Obj<Self>, now: Count) -> Result<Option<&'static str>, Error> {
        this.check_frozen()?;
        let reason = this.state.borrow_mut().batch.poll(now.duration());
        Ok(reason.map(|reason| match reason {
            FlushReason::Size => "size",
            _ => "deadline",
        }))
    }

    fn flush(this: Obj<Self>, now: Count) -> Result<Option<u32>, Error> {
        this.check_frozen()?;
        let count = this.state.borrow_mut().batch.flush(now.duration());
        Ok(count)
    }

    fn adapt(
        ruby: &Ruby,
        this: Obj<Self>,
        error: f64,
        elapsed: Count,
    ) -> Result<Option<u32>, Error> {
        this.check_frozen()?;
        if !error.is_finite() || elapsed.0 == 0 {
            return Err(Error::new(
                ruby.exception_arg_error(),
                "finite error and positive elapsed required",
            ));
        }
        let result = {
            let mut state = this.state.borrow_mut();
            let State { batch, pid } = &mut *state;
            batch.adapt(pid, error, elapsed.duration())
        };
        Ok(result)
    }

    fn pending(&self) -> u32 {
        self.state.borrow().batch.pending()
    }

    fn target(&self) -> u32 {
        self.state.borrow().batch.target()
    }
}

pub fn define(ruby: &Ruby, module: RModule) -> Result<(), Error> {
    let class = module.define_class("Batch", ruby.class_object())?;
    class.define_singleton_method("new", function!(Wrapper::new, 2))?;
    class.define_method("configure_pid", method!(Wrapper::configure_pid, 3))?;
    class.define_method("push", method!(Wrapper::push, 1))?;
    class.define_method("poll", method!(Wrapper::poll, 1))?;
    class.define_method("flush", method!(Wrapper::flush, 1))?;
    class.define_method("adapt", method!(Wrapper::adapt, 2))?;
    class.define_method("pending", method!(Wrapper::pending, 0))?;
    class.define_method("target", method!(Wrapper::target, 0))
}
