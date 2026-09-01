# SizeTrail

SizeTrail is a permanently read-only, audit-grade macOS storage attribution reporter for developers.

It explains Xcode, CoreSimulator, Homebrew, and Docker Desktop storage with explicit measurement
bases, uncertainty intervals, typed coverage gaps, and evidence-backed advice. It does not delete
files, execute advice, claim to reproduce Apple’s System Data total, or turn an unreadable region
into zero bytes.

The scan document uses schema version 1.0.0. Human wording is not a compatibility API; the typed
JSON signal set is the versioned machine surface.

## What it reports

- Capacity facts, each carrying its own measurement basis.
- Xcode build state, archives, DeviceSupport, simulator devices, and registered runtimes.
- Homebrew caches, logs, Cellar, Caskroom, and taps, without executing `brew`.
- Docker Desktop host disk images and daemon object sets, without summing those planes.
- Allocation intervals whose lower bound fails closed when required evidence is absent.
- Typed reasons for missing coverage, including unknown tool versions and policy-denied paths.
- Vendor commands or precise report paths as advice; SizeTrail never runs them.

Human output begins as each inventory stage is classified. `--json` waits for the scan to finish,
then emits one deterministic document.

```sh
sizetrail scan
sizetrail scan --json
sizetrail doctor --json
sizetrail rules --json
```

## Verified platforms

The table below is generated from the same source that expands the hosted CI matrix. A required row
is a release claim only when that release commit’s CI run is green. The API baseline is deliberately
separate from hosted runtime evidence.

<!-- BEGIN GENERATED: support-matrix -->
Release: **v1.0.0 stable schema**

API baseline: **macOS 13 best effort, not runtime-tested in hosted CI**

| Hosted lane | Architecture | Evidence status |
|---|---|---|
| macOS 15 Apple Silicon (`macos-15`) | `arm64` | required |
| macOS 15 Intel (`macos-15-intel`) | `x86_64` | required |
| macOS 26 Apple Silicon (`macos-26`) | `arm64` | required |
| macOS 26 Intel (`macos-26-intel`) | `x86_64` | required |
| Xcode 27 preview on macOS 26 Apple Silicon (`xcode-27`) | `arm64` | experimental; non-blocking |
| Real Xcode environment on macOS 15 Apple Silicon (`macos-15`) | `arm64` | real environment; non-blocking |
| Real Xcode environment on macOS 26 Apple Silicon (`macos-26`) | `arm64` | real environment; non-blocking |
<!-- END GENERATED: support-matrix -->

## Measurement example

This fragment is generated from the checked-in scan fixture; its quantities are not handwritten.

<!-- BEGIN GENERATED: fixture-report -->
The generated fixture reports `4096` bytes for `VolumeUsed` using `AttrVolSpaceUsed`.

It also reports `1` structured coverage gap and never derives a global remainder.
<!-- END GENERATED: fixture-report -->

The complete machine-readable fixture is [docs/generated/empty-scan.json](docs/generated/empty-scan.json).
The locked JSON Schema is [docs/generated/scan.schema.json](docs/generated/scan.schema.json).

## Measurement bases

<!-- BEGIN GENERATED: measurement-basis -->
Serialized measurement and capacity basis names locked from the compiled types.

## Measurement basis

- `logical_size`
- `allocated_footprint`
- `private_size`
- `volume_space_used`
- `vendor_reported`
- `docker_system_df`
- `private_floor_allocated_ceiling`

## Capacity basis

- `attr_vol_size`
- `attr_vol_space_used`
- `attr_vol_space_free`
- `attr_vol_space_available`
- `statfs_blocks`
- `statfs_blocks_minus_free`
- `statfs_free_blocks`
- `statfs_available_blocks`
- `core_foundation_important_usage`
- `core_foundation_opportunistic_usage`
<!-- END GENERATED: measurement-basis -->

## Coverage and unknown baseline

<!-- BEGIN GENERATED: coverage-unknown-baseline -->
Generated from the checked-in empty-scan fixture.

- schema version `1.0.0`
- findings: `0`
- coverage_gaps: `1`
- gap reasons: `no_adapters_compiled`
<!-- END GENERATED: coverage-unknown-baseline -->

## Read-only boundary

SizeTrail does not issue writes to user or system data paths. The repository enforces this with a
closed Rust API policy, a runtime tree snapshot harness, and a deny-write Seatbelt test on supported
hosted macOS runners. Platform loader registration to a character device is narrowly documented and
does not modify user or system data.

Read operations can still have side effects. SizeTrail never executes Xcode’s `simctl` wrapper because
that wrapper can invoke `xcodebuild -runFirstLaunch`; it calls the fixed CoreSimulator binary only after
an exact Xcode/CoreSimulator version match. The direct command may still start or connect to Apple’s
per-user CoreSimulator services. Docker probes use Docker Desktop’s bundled CLI after the
per-user Unix socket is verified, and may wake Resource Saver. Each external probe is a version-gated
registry entry with a hard call limit, timeout, disable switch, and typed list of known side effects.
`doctor` reports that list plus concrete target capabilities and errno; it does not claim to know a
global Full Disk Access state.

## Install

Download the archive for your architecture from the latest GitHub release, extract it, and place
`sizetrail` somewhere on your `PATH`. Release binaries are not notarized; inspect the checksums and
source before running them.

Shell completion is printed to stdout so installation remains your choice:

```sh
sizetrail completion zsh
```

## Contributing and license

Static attribution rules are compiled TOML. A rule contribution consists of rule data, evidence,
and its fixture; it does not require Rust or adapter-contract knowledge. Dynamic toolchain behavior
remains typed Rust code.

See [CONTRIBUTING.md](CONTRIBUTING.md). SizeTrail is dual-licensed under
[MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.

SizeTrail was inspired by the practical developer-storage problems surfaced by
[mole](https://github.com/tw93/Mole), but shares no code or prose with that GPL-licensed project.
