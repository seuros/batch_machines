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
