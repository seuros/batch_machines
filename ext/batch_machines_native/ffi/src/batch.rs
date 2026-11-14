use std::{cell::RefCell, sync::OnceLock, time::Duration};

use batch_machines::{Admission, Batch, FlushReason, Refusal};
use magnus::{
    Error, Float, RClass, RModule, Ruby, Symbol, TryConvert, Value, function, method, prelude::*,
    typed_data::Obj, value::Opaque,
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

/// An instant or span as Integer nanoseconds.
fn nanos(ruby: &Ruby, span: Duration) -> Value {
    ruby.integer_from_u128(span.as_nanos()).as_value()
}

fn type_error(ruby: &Ruby, message: &'static str) -> Error {
    Error::new(ruby.exception_type_error(), message)
}

fn range_error(ruby: &Ruby, message: &'static str) -> Error {
    Error::new(ruby.exception_range_error(), message)
}

/// What `Flush.new` stores: the core's `FlushReason`, as its symbol.
/// `TypeError` for a non-Symbol, `RangeError` for a symbol that names no
/// `FlushReason`.
fn checked_flush(ruby: &Ruby, value: Value) -> Result<Symbol, Error> {
    let symbol =
        Symbol::from_value(value).ok_or_else(|| type_error(ruby, "reason must be a Symbol"))?;
    let due = match symbol.name()?.as_ref() {
        "size" => FlushReason::Size,
        "deadline" => FlushReason::Deadline,
        _ => return Err(range_error(ruby, "reason must be :size or :deadline")),
    };
    Ok(reason(ruby, due))
}

/// `BatchMachines::Flush`, resolved once at init: swapping the constant later
/// changes nothing the batch builds.
static FLUSH: OnceLock<Opaque<RClass>> = OnceLock::new();

fn flush_class(ruby: &Ruby) -> Result<RClass, Error> {
    FLUSH
        .get()
        .map(|class| ruby.get_inner(*class))
        .ok_or_else(|| {
            Error::new(
                ruby.exception_runtime_error(),
                "the batch_machines extension is not initialised",
            )
        })
}

#[magnus::wrap(class = "BatchMachines::Batch", free_immediately, size)]
struct Wrapper(RefCell<Batch>);

impl Wrapper {
    /// Raises `RangeError` for a zero `max_wait` (the core answers `None`).
    fn new(ruby: &Ruby, capacity: Cost, max_wait: Nanos) -> Result<Self, Error> {
        Batch::new(capacity.0, max_wait.0)
            .map(|batch| Self(RefCell::new(batch)))
            .ok_or_else(|| range_error(ruby, "max_wait must be positive"))
    }

    /// A push answers with a `Flush` built by Ruby code, so it is counted on
    /// a copy and the batch moves only once the answer exists: a raise while
    /// building it counts nothing. A build that froze the batch meanwhile
    /// raises `FrozenError`, one that changed it (a redefined `Flush.new`
    /// calling back in) `RuntimeError`; neither applies the copy.
    fn admit(
        ruby: &Ruby,
        this: Obj<Self>,
        act: impl FnOnce(&mut Batch) -> Result<Admission, Refusal>,
    ) -> Result<Value, Error> {
        this.check_frozen()?;
        let flush = flush_class(ruby)?;
        let before = this.0.borrow().clone();
        let mut after = before.clone();
        let value = match act(&mut after) {
            Ok(Admission::Buffered) => ruby.to_symbol("buffered").as_value(),
            Ok(Admission::Flush(due)) => flush.funcall("new", (reason(ruby, due),))?,
            Err(refused) => refusal(ruby, refused),
        };
        this.check_frozen()?;
        let mut batch = this.0.borrow_mut();
        if *batch != before {
            return Err(Error::new(
                ruby.exception_runtime_error(),
                "the batch changed while its answer was built; the push was not applied",
            ));
        }
        *batch = after;
        Ok(value)
    }

    fn push(ruby: &Ruby, this: Obj<Self>, now: Nanos) -> Result<Value, Error> {
        Self::admit(ruby, this, |batch| batch.push(now.0))
    }

    fn push_weighted(ruby: &Ruby, this: Obj<Self>, now: Nanos, cost: Cost) -> Result<Value, Error> {
        Self::admit(ruby, this, |batch| batch.push_weighted(now.0, cost.0))
    }

    fn poll(ruby: &Ruby, this: Obj<Self>, now: Nanos) -> Result<Option<Value>, Error> {
        this.check_frozen()?;
        let answer = this.0.borrow_mut().poll(now.0);
        Ok(match answer {
            Ok(due) => due.map(|due| reason(ruby, due).as_value()),
            Err(refused) => Some(refusal(ruby, refused)),
        })
    }

    /// A mutating call that answers a count: nothing can raise once the
    /// core has acted.
    fn count(
        ruby: &Ruby,
        this: Obj<Self>,
        act: impl FnOnce(&mut Batch) -> Result<u32, Refusal>,
    ) -> Result<Value, Error> {
        this.check_frozen()?;
        let answer = act(&mut this.0.borrow_mut());
        Ok(counted(ruby, answer))
    }

    fn flush(ruby: &Ruby, this: Obj<Self>, now: Nanos) -> Result<Value, Error> {
        Self::count(ruby, this, |batch| batch.flush(now.0))
    }

    /// The core's `adapt`, across two native extensions: the controller is
    /// the caller's object, stepped through its own `step`, and its answer is
    /// handed to the core's `retarget`. `adapt` is exactly that pair. The
    /// batch is checked for frozen before the controller is touched; a
    /// `step` that freezes it has stepped the caller's controller, which the
    /// binding cannot undo, and `retarget` then raises `FrozenError` with the
    /// batch unchanged.
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
                _ => {
                    let message = "step answered a Symbol that is not a controller refusal";
                    return Err(range_error(ruby, message));
                }
            };
            return Ok(refusal(ruby, refused));
        }
        let output = Float::from_value(stepped)
            .ok_or_else(|| type_error(ruby, "step must answer a Float or a refusal Symbol"))?;
        Self::retarget(ruby, this, output.to_f64())
    }

    fn retarget(ruby: &Ruby, this: Obj<Self>, output: f64) -> Result<Value, Error> {
        Self::count(ruby, this, |batch| batch.retarget(output))
    }

    fn capacity(&self) -> u32 {
        self.0.borrow().capacity()
    }

    fn max_wait(ruby: &Ruby, this: &Self) -> Value {
        nanos(ruby, this.0.borrow().max_wait())
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
        deadline.map(|deadline| nanos(ruby, deadline))
    }
}

pub fn define(ruby: &Ruby, module: RModule) -> Result<(), Error> {
    let flush: RClass = module.const_get("Flush")?;
    // Pinned: compaction must not move what the static points at.
    ruby.gc_register_mark_object(flush);
    FLUSH.set(flush.into()).map_err(|_| {
        Error::new(
            ruby.exception_runtime_error(),
            "the batch_machines extension was initialised twice",
        )
    })?;
    module.define_singleton_method("__flush", function!(checked_flush, 1))?;
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
