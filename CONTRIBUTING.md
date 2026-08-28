# Contributing to SizeTrail

SizeTrail treats measurement honesty and permanent read-only behavior as correctness constraints.
Read `AGENTS.md`, then `decisions.md`, then `SPEC.md` before changing behavior. A decision conflict
must be resolved in the decision record and specification before code diverges.

Use Rust 1.98.0 and locked dependencies:

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all
```

Static rule contributions should change the compiled TOML and add a fixture with evidence. They
must not add a command field. New dynamic probes require a typed adapter change, a version range,
a side-effect registry entry, and a channel-matrix update.

By submitting a contribution, you agree that it is licensed under either the MIT License or the
Apache License, Version 2.0, at each downstream recipient’s option (`MIT OR Apache-2.0`).
