#![allow(clippy::expect_used)]
use super::*;

mod model;

fn at(t: u64) -> Duration {
    Duration::from_secs(t)
}
fn nz(x: u32) -> NonZeroU32 {
    NonZeroU32::new(x).expect("positive")
}
fn batch(capacity: u32, max_wait: u64) -> Batch {
    Batch::new(nz(capacity), at(max_wait)).expect("configuration")
}
fn pid(max: f64) -> Pid {
    Pid::new(
        Gains::new(1.0, 0.0, 0.0).expect("gains"),
        Limits::new(0.0, max).expect("limits"),
    )
}
const SIZE: Result<Admission, Refusal> = Ok(Admission::Flush(FlushReason::Size));
const DEADLINE: Result<Admission, Refusal> = Ok(Admission::Flush(FlushReason::Deadline));
const BUFFERED: Result<Admission, Refusal> = Ok(Admission::Buffered);

#[test]
fn zero_max_wait_is_not_a_configuration() {
    assert_eq!(Batch::new(nz(1), Duration::ZERO), None);
    let batch = batch(3, 5);
    assert_eq!((batch.capacity(), batch.max_wait()), (3, at(5)));
    assert_eq!(
        (batch.target(), batch.pending(), batch.deadline()),
        (3, 0, None)
    );
}

#[test]
fn first_item_deadline_is_not_extended_by_arrivals() {
    let mut batch = batch(3, 5);
    assert_eq!(batch.push(at(1)), BUFFERED);
    assert_eq!(batch.push(at(4)), BUFFERED);
    assert_eq!(batch.poll(at(6)), Ok(Some(FlushReason::Deadline)));
    assert_eq!(batch.push(at(6)), DEADLINE);
    assert_eq!(batch.push(at(6)), Err(Refusal::Full));
    assert_eq!(batch.flush(at(6)), Ok(3));
    assert_eq!(batch.poll(at(6)), Ok(None));
}

#[test]
fn adaptation_changes_size_but_not_age() {
    let mut batch = batch(5, 10);
    let mut pid = pid(100.0);
    assert_eq!(batch.adapt(&mut pid, 2.0, at(1)), Ok(2));
    assert_eq!(batch.push(at(0)), BUFFERED);
    assert_eq!(batch.push(at(1)), SIZE);
    assert_eq!(batch.adapt(&mut pid, 100.0, at(1)), Ok(5));
    assert_eq!(batch.deadline(), Some(at(10)));
    assert_eq!(batch.poll(at(10)), Ok(Some(FlushReason::Deadline)));
    assert_eq!(batch.flush(at(10)), Ok(2));
    assert_eq!(batch.target(), 5);
}

#[test]
fn deadline_is_absolute_and_cleared_by_flush() {
    let mut batch = batch(4, 2);
    assert_eq!(batch.deadline(), None);
    assert_eq!(batch.push(at(3)), BUFFERED);
    assert_eq!(batch.push(at(4)), BUFFERED);
    assert_eq!(batch.deadline(), Some(at(5)));
    assert_eq!(batch.poll(at(4)), Ok(None));
    assert_eq!(batch.poll(at(5)), Ok(Some(FlushReason::Deadline)));
    assert_eq!(batch.flush(at(5)), Ok(2));
    assert_eq!((batch.deadline(), batch.pending()), (None, 0));
    assert_eq!(batch.push(at(6)), BUFFERED);
    assert_eq!(batch.deadline(), Some(at(8)));
}

#[test]
fn weighted_items_flush_at_the_cost_budget() {
    let mut batch = batch(10, 5);
    assert_eq!(batch.push_weighted(at(0), nz(4)), BUFFERED);
    assert_eq!(batch.push_weighted(at(1), nz(7)), Err(Refusal::Full));
    assert_eq!(batch.push_weighted(at(1), nz(6)), SIZE);
    assert_eq!((batch.pending(), batch.pending_cost()), (2, 10));
    assert_eq!(batch.push(at(1)), Err(Refusal::Full));
    assert_eq!(batch.flush(at(2)), Ok(2));
    assert_eq!(batch.pending_cost(), 0);
    assert_eq!(batch.push_weighted(at(2), nz(10)), SIZE);
    assert_eq!(batch.flush(at(2)), Ok(1));
}

#[test]
fn adapted_target_counts_cost_not_items() {
    let mut batch = batch(100, 10);
    let mut pid = pid(1000.0);
    assert_eq!(batch.adapt(&mut pid, 8.0, at(1)), Ok(8));
    assert_eq!(batch.push_weighted(at(0), nz(5)), BUFFERED);
    assert_eq!(batch.push_weighted(at(0), nz(3)), SIZE);
    assert_eq!(batch.flush(at(0)), Ok(2));
    assert_eq!(batch.adapt(&mut pid, 1000.0, at(1)), Ok(100));
    assert_eq!(batch.push_weighted(at(1), nz(99)), BUFFERED);
    assert_eq!(batch.push(at(1)), SIZE);
    assert_eq!(batch.flush(at(1)), Ok(2));
}

/// The defect this crate shipped with: a refused push recorded its clock, so
/// a later, earlier-but-valid call was refused as a regression.
#[test]
fn a_refused_push_does_not_observe_its_clock() {
    let mut batch = batch(2, 1);
    assert_eq!(batch.push_weighted(at(5), nz(3)), Err(Refusal::Full));
    assert_eq!(batch.push(at(4)), BUFFERED);
    assert_eq!(batch.flush(at(4)), Ok(1));
    assert_eq!(batch.push(Duration::MAX), Err(Refusal::Overflow));
    assert_eq!(batch.push(at(6)), BUFFERED);
}

#[test]
fn every_refusal_changes_nothing() {
    let mut batch = batch(4, 2);
    assert_eq!(batch.push_weighted(at(5), nz(3)), BUFFERED);
    let before = batch.clone();
    assert_eq!(batch.push(at(4)), Err(Refusal::TimeRegression));
    assert_eq!(batch.poll(at(4)), Err(Refusal::TimeRegression));
    assert_eq!(batch.flush(at(4)), Err(Refusal::TimeRegression));
    assert_eq!(batch.push_weighted(at(9), nz(2)), Err(Refusal::Full));
    assert_eq!(batch.retarget(f64::NAN), Err(Refusal::NotFinite));
    assert_eq!(batch.retarget(f64::INFINITY), Err(Refusal::NotFinite));
    assert_eq!(batch, before);

    // Overflow needs an empty batch: a pending deadline is never recomputed.
    assert_eq!(batch.push(Duration::MAX), DEADLINE);
    assert_eq!(batch.flush(Duration::MAX), Ok(2));
    let before = batch.clone();
    assert_eq!(batch.push(Duration::MAX), Err(Refusal::Overflow));
    assert_eq!(batch, before);
}

#[test]
fn a_refused_adapt_changes_neither_batch_nor_controller() {
    let mut batch = batch(5, 10);
    let mut huge = Pid::new(
        Gains::new(1.0e308, 0.0, 0.0).expect("gains"),
        Limits::new(0.0, 5.0).expect("limits"),
    );
    let mut pid = pid(5.0);
    assert_eq!(batch.adapt(&mut pid, 2.0, at(1)), Ok(2));
    let (before, controller, blown) = (batch.clone(), pid.clone(), huge.clone());
    assert_eq!(
        batch.adapt(&mut pid, f64::NAN, at(1)),
        Err(Refusal::NotFinite)
    );
    assert_eq!(
        batch.adapt(&mut pid, 1.0, Duration::ZERO),
        Err(Refusal::InvalidDuration)
    );
    assert_eq!(
        batch.adapt(&mut huge, 1.0e308, at(1)),
        Err(Refusal::Overflow)
    );
    assert_eq!((&batch, &pid, &huge), (&before, &controller, &blown));
}

#[test]
fn adapt_is_step_then_retarget() {
    let gains = Gains::new(0.5, 1.0, 0.25).expect("gains");
    let limits = Limits::new(0.0, 50.0).expect("limits");
    let (mut whole, mut halves) = (batch(20, 10), batch(20, 10));
    let (mut stepped, mut split) = (Pid::new(gains, limits), Pid::new(gains, limits));
    for error in [3.0, -40.0, 7.5, 100.0, 0.2] {
        let output = split.step(error, at(1)).expect("finite");
        assert_eq!(
            whole.adapt(&mut stepped, error, at(1)),
            halves.retarget(output)
        );
        assert_eq!((&whole, &stepped), (&halves, &split));
    }
}

#[test]
fn retarget_truncates_and_clamps_to_capacity() {
    let mut batch = batch(5, 10);
    assert_eq!(batch.retarget(2.9), Ok(2));
    assert_eq!(batch.retarget(-100.0), Ok(1));
    assert_eq!(batch.retarget(0.0), Ok(1));
    assert_eq!(batch.retarget(1.0e300), Ok(5));
    let before = batch.clone();
    assert_eq!(batch.retarget(5.5), Ok(5));
    assert_eq!(batch, before);
}

#[test]
fn honoured_repeats_change_nothing() {
    let mut batch = batch(3, 5);
    assert_eq!(batch.push(at(1)), BUFFERED);
    assert_eq!(batch.flush(at(2)), Ok(1));
    let before = batch.clone();
    assert_eq!(batch.flush(at(2)), Ok(0));
    assert_eq!(batch.poll(at(2)), Ok(None));
    assert_eq!(batch, before);
}

#[test]
fn refusal_is_an_error_with_a_message_per_variant() {
    let all = [
        Refusal::TimeRegression,
        Refusal::Overflow,
        Refusal::Full,
        Refusal::NotFinite,
        Refusal::InvalidDuration,
    ];
    let error: &dyn core::error::Error = &all[0];
    assert!(error.source().is_none());
    for (i, refusal) in all.iter().enumerate() {
        for other in &all[i + 1..] {
            assert_ne!(refusal, other);
        }
    }
}
