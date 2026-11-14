# batch-machines

> **AMS Nagle**, NCC-4202, *Nagle*-class cargo consolidator.

Engine-room flight manual: fill the cargo hold without missing the launch window.

Allocation-free `no_std` size/deadline batch decisions. The first item establishes
a fixed maximum-wait deadline; later arrivals and target adaptation never move
that deadline. Poll with explicit monotonic time, or read `deadline()` and sleep
until it: it is the absolute time at which `poll` first reports `Deadline`, and
`None` while nothing is pending. A flush recommendation does not flush payloads:
the adapter must execute/acknowledge the flush, which also clears the deadline.

Capacity is a budget of cost units. `push` spends one unit per item;
`push_weighted` spends the item's cost, so a byte budget flushes at a byte
target. An item costing more than the capacity is never admitted. Flow credits
bound the pending cost. PID output can adapt the target cost, truncated toward
zero and clamped to `[1, capacity]`. Error definition, gains,
measurement, and downstream flush concurrency are caller policies. This cannot
guarantee an end-to-end latency budget if timers/execution are delayed. No I/O,
timer driver, payload storage, partial flush, or Ruby binding is included.

## Contract

- **Answers.** Every operation that can refuse returns `Result<T, Refusal>`,
  and `T` describes only honoured outcomes.

  | Call | Honoured | Records `now` | Refused |
  | --- | --- | --- | --- |
  | `Batch::new(capacity, max_wait)` | `Some(Batch)` | no clock | `None` for a zero `max_wait` (configuration) |
  | `push(now)`, `push_weighted(now, cost)` | `Admission::Buffered`, `Admission::Flush(FlushReason)`; the item is counted either way | yes | `TimeRegression`, `Overflow`, `Full` |
  | `poll(now)` | `Some(FlushReason)`, or `None` when nothing is due at this instant | yes, `None` included | `TimeRegression` |
  | `flush(now)` | the items that were pending, `0` for an empty batch | yes when items left; `0` records nothing | `TimeRegression` |
  | `adapt(pid, error, elapsed)` | the new target | no, `elapsed` is a span | `NotFinite`, `InvalidDuration`, `Overflow` |
  | `retarget(output)` | the new target | no clock | `NotFinite` |

- **Refusals change nothing.** Every `Err` leaves the batch equal to a clone
  taken before the call: no item counted, no cost held, and the `now` it
  carried is not observed, so a refused call can never make a later one a
  `TimeRegression`. A refused `adapt` leaves the controller untouched too.
- **`Refusal`.** `TimeRegression`: `now` is earlier than the latest instant an
  honoured `push`, `poll` or non-empty `flush` observed. `Overflow`: `now + max_wait`
  does not fit a `Duration` (only a push that would start a batch computes
  it), or a term of the PID calculation is not finite. `Full`: the item costs
  more than the capacity still unspent. `NotFinite`: the error or output is
  NaN or infinite. `InvalidDuration`: `adapt` was given a zero `elapsed`.
- **Which answers record the clock.** An honoured call records `now` exactly
  when its answer depends on `now`: every honoured push (its admission names
  what is due at that instant), every honoured `poll` (`None` is "nothing due
  at this instant"), and a `flush` that emptied the batch. An empty `flush`
  answers `0` whatever the time and records nothing. `adapt` and `retarget`
  read no clock.
- **No unreachable refusals.** A push asks the credit window what is
  `available()` and only then acquires, so no `flow-machines` refusal is
  translated: `Full` is the batch's own answer. The acquisition that follows
  cannot be refused; that invariant is a `debug_assert!`, and a release build
  answers `Full` with nothing changed. Nothing in the crate panics.
- **Repeats.** `flush` on an empty batch is honoured and answers `0`; the
  batch can prove it, because nothing is pending. It changes nothing at any
  `now`. A repeated `poll` at the same `now` answers the same and changes
  nothing. `retarget` to the target already set changes nothing. A push is
  never a repeat: the batch counts items, it does not know them.
- **Time.** Instants and spans are `core::time::Duration` on the caller's
  monotonic clock. The batch reads no clock.
- **No issued ids.** The batch hands out no handle, so there is no scope and
  nothing to resume: a restarted batch is `Batch::new` again, empty.
- **Sibling seams.** `Pid`, `Gains` and `Limits` are `feedback-machines`
  types, re-exported here because `adapt` names `Pid`. `adapt` steps the
  caller's controller and aims the target at its output; it is exactly
  `pid.step(error, elapsed)` followed by `retarget(output)`. It is not named
  `adapt_from`: that name is for a method that only reads a sibling object,
  and this one moves the controller's integral and derivative history.
  `retarget` is the same aiming for a caller that computes the output some
  other way. The `flow-machines` credit window is private.
