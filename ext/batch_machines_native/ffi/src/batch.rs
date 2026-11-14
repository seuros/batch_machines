use std::cell::RefCell;

use batch_machines::{Admission, Batch, FlushReason, Refusal};
use magnus::{
    Error, Float, RModule, Ruby, Symbol, Value, function, method, prelude::*, typed_data::Obj,
};
use matryona_machines::ruby::{
    Class, Registry, Symbols, duration, keep, nanoseconds, positive, real, symbol,
};

matryona_machines::symbols! {
    /// The core's refusals, each the snake case of its variant.
    Refusals for Refusal as "a refusal" {
        TimeRegression => "time_regression",
        Overflow => "overflow",
        Full => "full",
        NotFinite => "not_finite",
        InvalidDuration => "invalid_duration",
    }
}

/// Ruby symbol of a refusal.
fn refusal(ruby: &Ruby, refused: Refusal) -> Value {
    Refusals::symbol(ruby, refused).as_value()
}

matryona_machines::symbols! {
    /// Why a batch flushed, as `Flush#reason` holds it.
    Reasons for FlushReason as "reason" {
        Size => "size",
        Deadline => "deadline",
    }
}

matryona_machines::symbols! {
    /// The refusals a `feedback_machines` controller's `step` answers.
    ControllerRefusals for feedback_machines::Refusal as "step's refusal" {
        NotFinite => "not_finite",
        InvalidDuration => "invalid_duration",
        Overflow => "overflow",
    }
}

fn reason(ruby: &Ruby, reason: FlushReason) -> Symbol {
    Reasons::symbol(ruby, reason)
}

/// The controller's refusal as the core's `adapt` carries it.
const fn controller_refusal(refused: feedback_machines::Refusal) -> Refusal {
    match refused {
        feedback_machines::Refusal::NotFinite => Refusal::NotFinite,
        feedback_machines::Refusal::InvalidDuration => Refusal::InvalidDuration,
        feedback_machines::Refusal::Overflow => Refusal::Overflow,
    }
}

/// A count, or the refusal symbol.
fn counted(ruby: &Ruby, answer: Result<u32, Refusal>) -> Value {
    match answer {
        Ok(count) => ruby.integer_from_u64(u64::from(count)).as_value(),
        Err(refused) => refusal(ruby, refused),
    }
}

/// What `Flush.new` stores: the core's `FlushReason`, as its symbol.
/// `TypeError` for a non-Symbol, `RangeError` for a symbol that names no
/// `FlushReason`.
fn checked_flush(ruby: &Ruby, value: Value) -> Result<Symbol, Error> {
    let due = Reasons::parse(ruby, value)?;
    Ok(reason(ruby, due))
}

/// `BatchMachines::Flush`, resolved once at init: swapping the constant later
/// changes nothing the batch builds.
struct Classes {
    flush: Class,
}

static CLASSES: Registry<Classes> = Registry::new("batch_machines");

#[magnus::wrap(class = "BatchMachines::Batch", free_immediately, size)]
struct Wrapper(RefCell<Batch>);

impl Wrapper {
    /// Raises `RangeError` for a zero `max_wait` (the core answers `None`).
    fn new(ruby: &Ruby, capacity: Value, max_wait: Value) -> Result<Self, Error> {
        let capacity = positive(capacity, "capacity")?;
        Batch::new(capacity, duration(max_wait, "max_wait")?)
            .map(|batch| Self(RefCell::new(batch)))
            .ok_or_else(|| Error::new(ruby.exception_range_error(), "max_wait must be positive"))
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
        let flush = CLASSES.get(ruby)?.flush.get(ruby);
        keep(ruby, this, &this.0, act, |admitted| match admitted {
            Ok(Admission::Buffered) => Ok(symbol(ruby, "buffered").as_value()),
            Ok(Admission::Flush(due)) => flush.funcall("new", (reason(ruby, due),)),
            Err(refused) => Ok(refusal(ruby, refused)),
        })
    }

    fn push(ruby: &Ruby, this: Obj<Self>, now: Value) -> Result<Value, Error> {
        this.check_frozen()?;
        let now = duration(now, "now")?;
        Self::admit(ruby, this, |batch| batch.push(now))
    }

    fn push_weighted(
        ruby: &Ruby,
        this: Obj<Self>,
        now: Value,
        cost: Value,
    ) -> Result<Value, Error> {
        this.check_frozen()?;
        let (now, cost) = (duration(now, "now")?, positive(cost, "cost")?);
        Self::admit(ruby, this, |batch| batch.push_weighted(now, cost))
    }

    fn poll(ruby: &Ruby, this: Obj<Self>, now: Value) -> Result<Option<Value>, Error> {
        this.check_frozen()?;
        let now = duration(now, "now")?;
        let answer = this.0.borrow_mut().poll(now);
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

    fn flush(ruby: &Ruby, this: Obj<Self>, now: Value) -> Result<Value, Error> {
        this.check_frozen()?;
        let now = duration(now, "now")?;
        Self::count(ruby, this, |batch| batch.flush(now))
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
        real(error, "error")?;
        duration(elapsed, "elapsed")?;
        let stepped: Value = pid.funcall("step", (error, elapsed))?;
        if Symbol::from_value(stepped).is_some() {
            // The controller's refusals, by name: a `feedback_machines::Refusal`.
            let refused = ControllerRefusals::parse(ruby, stepped)?;
            return Ok(refusal(ruby, controller_refusal(refused)));
        }
        let output = Float::from_value(stepped).ok_or_else(|| {
            let message = "step must answer a Float or a refusal Symbol";
            Error::new(ruby.exception_type_error(), message)
        })?;
        Self::count(ruby, this, |batch| batch.retarget(output.to_f64()))
    }

    /// Frozen is checked before `output` is converted.
    fn retarget(ruby: &Ruby, this: Obj<Self>, output: Value) -> Result<Value, Error> {
        this.check_frozen()?;
        let output = real(output, "output")?;
        Self::count(ruby, this, |batch| batch.retarget(output))
    }

    fn capacity(&self) -> u32 {
        self.0.borrow().capacity()
    }

    fn max_wait(ruby: &Ruby, this: &Self) -> Value {
        nanoseconds(ruby, this.0.borrow().max_wait()).as_value()
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
        deadline.map(|deadline| nanoseconds(ruby, deadline).as_value())
    }
}

pub fn define(ruby: &Ruby, module: RModule) -> Result<(), Error> {
    // `Flush` is defined in Ruby before the extension loads: a missing one
    // is `NameError`.
    let flush = Class::resolve(ruby, module, "Flush")?;
    CLASSES.set(ruby, Classes { flush })?;
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
