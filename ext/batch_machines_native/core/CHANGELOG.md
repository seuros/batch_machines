# Changelog

## [0.4.0](https://github.com/seuros/batch_machines/compare/batch-machines-v0.3.0...batch-machines-v0.4.0) (2026-11-13)


### ⚠ BREAKING CHANGES

* converted to the fleet conventions. One exhaustive Refusal per crate, Result from every operation that can refuse, a refusal changes nothing, provable idempotent repeats are honoured, Ruby mirrors the core. No compatibility aliases. Requires flow-machines >= 0.3 and feedback-machines >= 0.3. configure_pid is gone; Ruby answers symbols and a Flush value.

### Features

* answer Result&lt;_, Refusal&gt;, stop refused pushes recording the clock, adapt a caller-owned controller ([a3746a5](https://github.com/seuros/batch_machines/commit/a3746a5c91bf437e38389cb5aee5022b3b760797))

## [0.3.0](https://github.com/seuros/batch_machines/compare/batch-machines-v0.2.2...batch-machines-v0.3.0) (2026-11-06)


### Features

* expose the pending deadline and weighted pushes ([9044fa3](https://github.com/seuros/batch_machines/commit/9044fa323859bcd6fb6cf9a7b5c884bf503fedc4))

## [0.2.2](https://github.com/seuros/batch_machines/compare/batch-machines-v0.2.1...batch-machines-v0.2.2) (2026-11-06)


### Bug Fixes

* crates.io keywords and categories for the core ([b150571](https://github.com/seuros/batch_machines/commit/b150571563871f7af013b716402aa169688deaf2))

## [0.2.1](https://github.com/seuros/batch_machines/compare/batch-machines-v0.2.0...batch-machines-v0.2.1) (2026-11-06)


### Bug Fixes

* keep test sources out of the packaged gem and crate ([2287c10](https://github.com/seuros/batch_machines/commit/2287c1089fefd9154740078a6977598f47033d0e))

## [0.2.0](https://github.com/seuros/batch_machines/compare/batch-machines-v0.1.0...batch-machines-v0.2.0) (2026-11-06)


### Features

* initial extraction ([da62e4b](https://github.com/seuros/batch_machines/commit/da62e4bb16f3d1eaecc93ebf2a739f86fcfee932))
