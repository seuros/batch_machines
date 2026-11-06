# batch-machines

> **AMS Nagle**, NCC-4202, *Nagle*-class cargo consolidator.

Engine-room flight manual: fill the cargo hold without missing the launch window.

Allocation-free `no_std` size/deadline batch decisions. The first item establishes
a fixed maximum-wait deadline; later arrivals and target adaptation never move
that deadline. Poll with explicit monotonic time. A flush recommendation does
not flush payloads: the adapter must execute/acknowledge the flush.

Flow credits bound pending item count. PID output can adapt the target size,
truncated toward zero and clamped to `[1, capacity]`. Error definition, gains,
measurement, and downstream flush concurrency are caller policies. This cannot
guarantee an end-to-end latency budget if timers/execution are delayed. No I/O,
timer driver, payload storage, partial flush, or Ruby binding is included.
