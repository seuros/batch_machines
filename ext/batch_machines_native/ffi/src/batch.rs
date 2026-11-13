use std::cell::RefCell;

use batch_machines::{Admission, Batch, FlushReason, Refusal};
use magnus::{
    Error, Float, RModule, Ruby, Symbol, TryConvert, Value, function, method, prelude::*,
    typed_data::Obj,
};

use crate::{Cost, Nanos};

/// Ruby symbol of a refusal: the snake case of the core variant.
fn refusal(ruby: &Ruby, refused: Refusal) -> Value {
    ruby.to_symbol(match refused {
        Refusal::TimeRegression => "time_regression",
        Refusal::Overflow => "overflow",
        Refusal::Full => "full",
        Refusal::NotFinite => "not_finite",
        Refusal::InvalidDuration => "invalid_duration",
    })
    .as_value()
}

fn reason(ruby: &Ruby, reason: FlushReason) -> Symbol {
    ruby.to_symbol(match reason {
        FlushReason::Size => "size",
        FlushReason::Deadline => "deadline",
    })
}

/// A count, or the refusal symbol.
fn counted(ruby: &Ruby, answer: Result<u32, Refusal>) -> Value {
    match answer {
        Ok(count) => ruby.integer_from_u64(u64::from(count)).as_value(),
        Err(refused) => refusal(ruby, refused),
    }
}

fn type_error(ruby: &Ruby, message: &'static str) -> Error {
    Error::new(ruby.exception_type_error(), message)
}

#[magnus::wrap(class = "BatchMachines::Batch", free_immediately, size)]
struct Wrapper(RefCell<Batch>);

impl Wrapper {
    /// Raises `RangeError` for a zero `max_wait` (the core answers `None`).
    fn new(ruby: &Ruby, capacity: Cost, max_wait: Nanos) -> Result<Self, Error> {
        Batch::new(capacity.0, max_wait.0)
            .map(|batch| Self(RefCell::new(batch)))
            .ok_or_else(|| Error::new(ruby.exception_range_error(), "max_wait must be positive"))
    }

    fn admitted(ruby: &Ruby, answer: Result<Admission, Refusal>) -> Result<Value, Error> {
        match answer {
            Ok(Admission::Buffered) => Ok(ruby.to_symbol("buffered").as_value()),
            Ok(Admission::Flush(due)) => ruby
                .define_module("BatchMachines")?
                .const_get::<_, Value>("Flush")?
                .funcall("new", (reason(ruby, due),)),
            Err(refused) => Ok(refusal(ruby, refused)),
        }
    }

    fn push(ruby: &Ruby, this: Obj<Self>, now: Nanos) -> Result<Value, Error> {
        this.check_frozen()?;
        let answer = this.0.borrow_mut().push(now.0);
        Self::admitted(ruby, answer)
    }

    fn push_weighted(ruby: &Ruby, this: Obj<Self>, now: Nanos, cost: Cost) -> Result<Value, Error> {
        this.check_frozen()?;
        let answer = this.0.borrow_mut().push_weighted(now.0, cost.0);
        Self::admitted(ruby, answer)
    }

    fn poll(ruby: &Ruby, this: Obj<Self>, now: Nanos) -> Result<Option<Value>, Error> {
        this.check_frozen()?;
        let answer = this.0.borrow_mut().poll(now.0);
        Ok(match answer {
            Ok(due) => due.map(|due| reason(ruby, due).as_value()),
            Err(refused) => Some(refusal(ruby, refused)),
        })
    }

    fn flush(ruby: &Ruby, this: Obj<Self>, now: Nanos) -> Result<Value, Error> {
        this.check_frozen()?;
        let answer = this.0.borrow_mut().flush(now.0);
        Ok(counted(ruby, answer))
    }

    /// The core's `adapt`, across two native extensions: the controller is
    /// the caller's object, stepped through its own `step`, and its answer is
    /// handed to the core's `retarget`. `adapt` is exactly that pair.
    fn adapt(
        ruby: &Ruby,
        this: Obj<Self>,
        pid: Value,
        error: Value,
        elapsed: Value,
    ) -> Result<Value, Error> {
        this.check_frozen()?;
        f64::try_convert(error)?;
        Nanos::try_convert(elapsed)?;
        let stepped: Value = pid.funcall("step", (error, elapsed))?;
        if let Some(symbol) = Symbol::from_value(stepped) {
            // The controller's refusals, by name: a `feedback_machines::Refusal`.
            let refused = match symbol.name()?.as_ref() {
                "not_finite" => Refusal::NotFinite,
                "invalid_duration" => Refusal::InvalidDuration,
                "overflow" => Refusal::Overflow,
                _ => return Err(type_error(ruby, "step answered an unknown refusal")),
            };
            return Ok(refusal(ruby, refused));
        }
        let output = Float::from_value(stepped)
            .ok_or_else(|| type_error(ruby, "step must answer a Float or a refusal Symbol"))?;
        let answer = this.0.borrow_mut().retarget(output.to_f64());
        Ok(counted(ruby, answer))
    }

    fn retarget(ruby: &Ruby, this: Obj<Self>, output: f64) -> Result<Value, Error> {
        this.check_frozen()?;
        let answer = this.0.borrow_mut().retarget(output);
        Ok(counted(ruby, answer))
    }

    fn capacity(&self) -> u32 {
        self.0.borrow().capacity()
    }

    fn max_wait(ruby: &Ruby, this: &Self) -> Value {
        let max_wait = this.0.borrow().max_wait();
        ruby.integer_from_u128(max_wait.as_nanos()).as_value()
    }

    fn pending(&self) -> u32 {
        self.0.borrow().pending()
    }

    fn pending_cost(&self) -> u32 {
        self.0.borrow().pending_cost()
    }

    fn target(&self) -> u32 {
        self.0.borrow().target()
    }

    fn deadline(ruby: &Ruby, this: &Self) -> Option<Value> {
        let deadline = this.0.borrow().deadline();
        deadline.map(|deadline| ruby.integer_from_u128(deadline.as_nanos()).as_value())
    }
}

pub fn define(ruby: &Ruby, module: RModule) -> Result<(), Error> {
    let class = module.define_class("Batch", ruby.class_object())?;
    class.undef_default_alloc_func();
    class.define_singleton_method("new", function!(Wrapper::new, 2))?;
    class.define_method("push", method!(Wrapper::push, 1))?;
    class.define_method("push_weighted", method!(Wrapper::push_weighted, 2))?;
    class.define_method("poll", method!(Wrapper::poll, 1))?;
    class.define_method("flush", method!(Wrapper::flush, 1))?;
    class.define_method("adapt", method!(Wrapper::adapt, 3))?;
    class.define_method("retarget", method!(Wrapper::retarget, 1))?;
    class.define_method("capacity", method!(Wrapper::capacity, 0))?;
    class.define_method("max_wait", method!(Wrapper::max_wait, 0))?;
    class.define_method("pending", method!(Wrapper::pending, 0))?;
    class.define_method("pending_cost", method!(Wrapper::pending_cost, 0))?;
    class.define_method("target", method!(Wrapper::target, 0))?;
    class.define_method("deadline", method!(Wrapper::deadline, 0))
}
