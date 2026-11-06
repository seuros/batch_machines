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
