//! Randomized run against a plain reference model.
use super::*;

/// The batch, restated without a credit window.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Model {
    capacity: u32,
    max_wait: Duration,
    items: u32,
    cost: u32,
    target: u32,
    deadline: Option<Duration>,
    observed: Duration,
}

impl Model {
    fn reason(&self, now: Duration) -> Option<FlushReason> {
        if self.deadline.is_some_and(|deadline| now >= deadline) {
            Some(FlushReason::Deadline)
        } else if self.cost >= self.target {
            Some(FlushReason::Size)
        } else {
            None
        }
    }

    fn push(&mut self, now: Duration, cost: u32) -> Result<Admission, Refusal> {
        if now < self.observed {
            return Err(Refusal::TimeRegression);
        }
        let deadline = match self.deadline {
            Some(deadline) => deadline,
            None => now.checked_add(self.max_wait).ok_or(Refusal::Overflow)?,
        };
        if cost > self.capacity - self.cost {
            return Err(Refusal::Full);
        }
        self.observed = now;
        self.items += 1;
        self.cost += cost;
        self.deadline = Some(deadline);
        Ok(self
            .reason(now)
            .map_or(Admission::Buffered, Admission::Flush))
    }

    fn poll(&mut self, now: Duration) -> Result<Option<FlushReason>, Refusal> {
        if now < self.observed {
            return Err(Refusal::TimeRegression);
        }
        self.observed = now;
        Ok(self.reason(now))
    }

    fn flush(&mut self, now: Duration) -> Result<u32, Refusal> {
        if now < self.observed {
            return Err(Refusal::TimeRegression);
        }
        if self.items == 0 {
            return Ok(0);
        }
        self.observed = now;
        self.cost = 0;
        self.deadline = None;
        Ok(core::mem::take(&mut self.items))
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn retarget(&mut self, output: f64) -> Result<u32, Refusal> {
        if !output.is_finite() {
            return Err(Refusal::NotFinite);
        }
        self.target = output.clamp(1.0, f64::from(self.capacity)) as u32;
        Ok(self.target)
    }

    fn agrees(&self, batch: &Batch) -> bool {
        (self.items, self.cost, self.target, self.deadline)
            == (
                batch.pending(),
                batch.pending_cost(),
                batch.target(),
                batch.deadline(),
            )
    }
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }
}

/// Near `Duration::MAX` one draw in sixteen, so `Overflow` is reached too.
fn instant(rng: &mut Rng, clock: u64) -> Duration {
    match rng.below(16) {
        0 => Duration::MAX,
        1..=3 => at(clock.saturating_sub(rng.below(4))),
        _ => at(clock + rng.below(3)),
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn run(seed: u64, refusals: &mut [u32; 5], repeats: &mut u32) {
    let mut rng = Rng(seed);
    let capacity = 1 + rng.below(12) as u32;
    let max_wait = at(1 + rng.below(6));
    let mut batch = Batch::new(nz(capacity), max_wait).expect("configuration");
    let mut model = Model {
        capacity,
        max_wait,
        items: 0,
        cost: 0,
        target: capacity,
        deadline: None,
        observed: Duration::ZERO,
    };
    let mut controller = pid(64.0);
    let mut clock = 0;
    for _ in 0..400 {
        let before = batch.clone();
        let now = instant(&mut rng, clock);
        let refused = match rng.below(8) {
            0..=2 => {
                let cost = 1 + rng.below(u64::from(capacity) + 2) as u32;
                let answer = batch.push_weighted(now, nz(cost));
                assert_eq!(answer, model.push(now, cost));
                answer.err()
            }
            3 => {
                let answer = batch.push(now);
                assert_eq!(answer, model.push(now, 1));
                answer.err()
            }
            4 => {
                let answer = batch.poll(now);
                assert_eq!(answer, model.poll(now));
                if answer.is_ok() && now == before.observed {
                    *repeats += 1;
                    assert_eq!(batch, before, "a repeated poll changes nothing");
                }
                answer.err()
            }
            5 => {
                let answer = batch.flush(now);
                assert_eq!(answer, model.flush(now));
                if answer == Ok(0) {
                    *repeats += 1;
                    assert_eq!(batch, before, "an empty flush changes nothing");
                }
                answer.err()
            }
            6 => {
                let output =
                    [f64::NAN, f64::INFINITY, -3.0, 0.4, 2.5, 7.9, 1.0e12][rng.below(7) as usize];
                let answer = batch.retarget(output);
                assert_eq!(answer, model.retarget(output));
                answer.err()
            }
            _ => {
                let error = [f64::NAN, -9.0, 0.5, 3.0, 11.0, 80.0][rng.below(6) as usize];
                let elapsed = at(rng.below(3));
                let saved = controller.clone();
                let mut twin = controller.clone();
                let answer = batch.adapt(&mut controller, error, elapsed);
                match twin.step(error, elapsed) {
                    Ok(output) => assert_eq!(answer, model.retarget(output)),
                    Err(_) => assert_eq!(controller, saved, "a refused adapt steps nothing"),
                }
                assert_eq!(controller, twin);
                answer.err()
            }
        };
        if let Some(refusal) = refused {
            assert_eq!(batch, before, "{refusal:?} changed the batch");
            refusals[refusal as usize] += 1;
        } else if now != Duration::MAX {
            clock = clock.max(now.as_secs());
        }
        assert!(model.agrees(&batch));
        assert_eq!(model.observed, batch.observed);
        // An honoured call at `Duration::MAX` pins the clock there: start over.
        if batch.observed == Duration::MAX {
            return;
        }
    }
}

/// Spreads consecutive seeds across the xorshift state (2^64 / golden ratio).
const SEED_SPREAD: u64 = 0x9E37_79B9_7F4A_7C15;

#[test]
fn random_runs_match_the_model_and_refusals_change_nothing() {
    let (mut refusals, mut repeats) = ([0; 5], 0);
    for seed in 1..=400_u64 {
        run(seed.wrapping_mul(SEED_SPREAD), &mut refusals, &mut repeats);
    }
    assert!(refusals.iter().all(|count| *count > 0), "{refusals:?}");
    assert!(repeats > 0);
}
