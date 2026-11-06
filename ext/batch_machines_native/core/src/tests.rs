#![allow(clippy::expect_used)]
use super::*;
use feedback_machines::{Gains, Limits};
fn at(t: u64) -> Duration {
    Duration::from_secs(t)
}
fn nz(x: u32) -> NonZeroU32 {
    NonZeroU32::new(x).expect("positive")
}

#[test]
fn first_item_deadline_is_not_extended_by_arrivals() {
    let mut batch = Batch::new(nz(3), at(5)).expect("configuration");
    assert_eq!(batch.push(at(1)), Admission::Buffered);
    assert_eq!(batch.push(at(4)), Admission::Buffered);
    assert_eq!(batch.poll(at(6)), Some(FlushReason::Deadline));
    assert_eq!(batch.push(at(6)), Admission::Flush(FlushReason::Deadline));
    assert_eq!(batch.push(at(6)), Admission::Full);
    assert_eq!(batch.flush(at(6)), Some(3));
    assert_eq!(batch.poll(at(6)), None);
}

#[test]
fn adaptation_changes_size_but_not_age_and_invalid_time_is_rejected() {
    let mut batch = Batch::new(nz(5), at(10)).expect("configuration");
    let mut pid = Pid::new(
        Gains::new(1.0, 0.0, 0.0).expect("gains"),
        Limits::new(0.0, 100.0).expect("limits"),
    );
    assert_eq!(batch.adapt(&mut pid, 2.0, at(1)), Some(2));
    assert_eq!(batch.push(at(0)), Admission::Buffered);
    assert_eq!(batch.push(at(1)), Admission::Flush(FlushReason::Size));
    assert_eq!(batch.adapt(&mut pid, 100.0, at(1)), Some(5));
    assert_eq!(batch.poll(at(10)), Some(FlushReason::Deadline));
    assert_eq!(batch.flush(at(9)), None);
    assert_eq!(batch.flush(at(10)), Some(2));
    assert_eq!(batch.push(Duration::MAX), Admission::Rejected);
}

#[test]
fn deadline_is_absolute_and_cleared_by_flush() {
    let mut batch = Batch::new(nz(4), at(2)).expect("configuration");
    assert_eq!(batch.deadline(), None);
    assert_eq!(batch.push(at(3)), Admission::Buffered);
    assert_eq!(batch.push(at(4)), Admission::Buffered);
    assert_eq!(batch.deadline(), Some(at(5)));
    assert_eq!(batch.poll(at(4)), None);
    assert_eq!(batch.poll(at(5)), Some(FlushReason::Deadline));
    assert_eq!(batch.flush(at(4)), None);
    assert_eq!(batch.deadline(), Some(at(5)));
    assert_eq!(batch.flush(at(5)), Some(2));
    assert_eq!(batch.deadline(), None);
    assert_eq!(batch.pending(), 0);
    assert_eq!(batch.flush(at(5)), Some(0));
    assert_eq!(batch.deadline(), None);
    assert_eq!(batch.push(at(6)), Admission::Buffered);
    assert_eq!(batch.deadline(), Some(at(8)));
}

#[test]
fn rejected_first_item_leaves_no_deadline() {
    let mut batch = Batch::new(nz(2), at(1)).expect("configuration");
    assert_eq!(batch.push_weighted(at(0), nz(3)), Admission::Full);
    assert_eq!(batch.deadline(), None);
    assert_eq!(batch.push(Duration::MAX), Admission::Rejected);
    assert_eq!(batch.deadline(), None);
}

#[test]
fn weighted_items_flush_at_the_cost_budget() {
    let mut batch = Batch::new(nz(10), at(5)).expect("configuration");
    assert_eq!(batch.push_weighted(at(0), nz(4)), Admission::Buffered);
    assert_eq!(batch.push_weighted(at(1), nz(7)), Admission::Full);
    assert_eq!(
        batch.push_weighted(at(1), nz(6)),
        Admission::Flush(FlushReason::Size)
    );
    assert_eq!(batch.pending(), 2);
    assert_eq!(batch.pending_cost(), 10);
    assert_eq!(batch.push(at(1)), Admission::Full);
    assert_eq!(batch.flush(at(2)), Some(2));
    assert_eq!(batch.pending_cost(), 0);
    assert_eq!(
        batch.push_weighted(at(2), nz(10)),
        Admission::Flush(FlushReason::Size)
    );
    assert_eq!(batch.flush(at(2)), Some(1));
}

#[test]
fn adapted_target_counts_cost_not_items() {
    let mut batch = Batch::new(nz(100), at(10)).expect("configuration");
    let mut pid = Pid::new(
        Gains::new(1.0, 0.0, 0.0).expect("gains"),
        Limits::new(0.0, 1000.0).expect("limits"),
    );
    assert_eq!(batch.adapt(&mut pid, 8.0, at(1)), Some(8));
    assert_eq!(batch.push_weighted(at(0), nz(5)), Admission::Buffered);
    assert_eq!(
        batch.push_weighted(at(0), nz(3)),
        Admission::Flush(FlushReason::Size)
    );
    assert_eq!(batch.pending(), 2);
    assert_eq!(batch.flush(at(0)), Some(2));
    assert_eq!(batch.adapt(&mut pid, 1000.0, at(1)), Some(100));
    assert_eq!(batch.push_weighted(at(1), nz(99)), Admission::Buffered);
    assert_eq!(batch.push(at(1)), Admission::Flush(FlushReason::Size));
    assert_eq!(batch.flush(at(1)), Some(2));
}
