# Temnion

**A standalone, Rust-first exact-state and history engine, starting with a small
in-memory foundation.**

Temnion separates live state from append-only evidence. Its long-term architecture
adds durable history, deterministic reconstruction, derived knowledge (EKS),
versioned transformations, and carefully gated evolution. Tzeentch is a planned
first consumer, not a dependency of the database core.

> **Current status: M0 foundation + M1 packed state + M2 in-memory event history.**
> This is not yet a durable database. All state and events disappear when their
> owning process or objects are dropped. Successful append means volatile
> admission, never a disk-synchronized commit. No performance target or native
> Jetson qualification is claimed.

## What works now

| Package | Implemented foundation |
| --- | --- |
| `temnion-core` | Shard/entity identities, independent source/epoch/sequence event IDs, clock-domain timestamps, valid/observed/known time and clock-scoped ranges |
| `temnion-state` | Generic dense `StateSlab<T>`, generation-safe slot reuse, bounded capacity, checked access and removal |
| `temnion-events` | Per-source bounded `EventLog<T>`, typed payloads, atomic batch admission, entity/time/known-as-of filters and bounded snapshot pagination |
| `temnion-cli` | `tem version`, `tem describe` (JSON capabilities), `tem demo`, help and nonzero errors |
| `temnion-bench` | Seeded A/B/D baseline workloads against explicitly labeled `Vec`/`HashMap` reference paths |

History queries currently scan an in-memory source-local snapshot. There is no
spatial/history index, persistent ID allocator, schema registry, distributed
transaction, automatic state materializer, or historical replay engine.

**Not implemented:** WAL, TSF, recovery, checkpoints/replay, branches, causal
graphs, typed query IR, TemQL/Tem parsers, `temniond`, TNP, IPC/C/Arrow/Flight, SQL,
MCP, Studio, EKS, transformations, evolution, or a Tzeentch adapter.

## Quick start

Install [Rust through rustup](https://rustup.rs/) and the platform's native linker.
Windows MSVC builds need Visual Studio C++ Build Tools; Linux builds need the
usual C compiler/linker development packages. No Node, npm, GUI, model runtime,
CUDA, or service is needed.

From the repository root:

```text
cargo run -p temnion-cli -- version
cargo run -p temnion-cli -- describe
cargo run -p temnion-cli -- demo
```

The scripted demo writes no files. It illustrates typed events, late evidence
excluded by a known-time cutoff, bounded history pages, and stale-handle
rejection. Its current state follows **arrival order**; this is not a durable
replay demonstration or a general temporal materialization policy.

Runnable embedded examples:

```text
cargo run -p temnion-state --example packed_state
cargo run -p temnion-events --example history
cargo doc --workspace --no-deps
```

## Toolchain and platforms

The workspace uses Rust edition 2024, resolver 3, and MSRV **1.85**.
`rust-toolchain.toml` pins **1.85.0** with rustfmt and Clippy. Project source
forbids unsafe code and the current Cargo dependency graph is std-only apart
from local workspace crates.

| Target goal | Foundation validation policy |
| --- | --- |
| Windows x64, MSVC | CI configured for native formatting, Clippy and tests on `windows-2022` |
| Linux x64, GNU | CI configured for native formatting, Clippy and tests on `ubuntu-22.04` |
| Linux ARM64, GNU | CI cross-**checks** Rust code only; no ARM64 linking or execution in that job |
| NVIDIA AGX Orin Developer Kit | JetPack 6.2.2 / L4T 36.5 / Ubuntu 22.04 is the native qualification goal, not a verified result |

CI checks both pinned 1.85.0 and latest stable on x64. Native ARM64 tests,
performance, userspace compatibility, and eventual native ARM64 Studio remain
required future qualification; a cross-check is not a substitute.

## Development and measurement

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run --release -p temnion-bench -- --entities 4096 --events 100000 --iterations 100000
```

The benchmark is a volatile, in-process baseline, not evidence of durable
database throughput. Aggregate `ns/op` is not a latency percentile. See
[BENCHMARKING](docs/BENCHMARKING.md) for scope, lower-bound comparisons, and the
full A–L measurement plan.

## Documentation

- [Architecture](docs/ARCHITECTURE.md): current boundaries and the complete design.
- [Roadmap](docs/ROADMAP.md): R0–R6, T00–T21, all source M0–M44, and Gates A/B/C.
- [Core contract ADR](docs/adr/0001-foundation-contracts.md): identities, clocks,
  state, volatile append/query semantics, and requirements before persistence.
- [Compatibility](docs/COMPATIBILITY.md): Rust API, toolchains, future formats,
  protocols, upgrades, and release evidence.
- [Dependency and license inventory](docs/DEPENDENCIES.md).
- [Original architecture artifacts](docs/architecture/source/README.md):
  unchanged historical sources, not documentation of shipped interfaces.
- [Security](SECURITY.md) and [contributing](CONTRIBUTING.md).

Outside code contributions are **not being accepted** until the owner approves
CLA/relicensing terms. Bug reports and design feedback are welcome; a DCO alone
does not establish commercial relicensing rights.

## License

This project is dual-licensed:

- **AGPL-3.0** for open-source use under the terms in [`LICENSE`](./LICENSE).
- **Commercial license** for proprietary, closed-source, OEM, SaaS, or other
  uses requiring separately negotiated rights. See
  [`COMMERCIAL_LICENSE.md`](./COMMERCIAL_LICENSE.md).

Commercial licensing: **willaveryvy@gmail.com**

The public source licensing path is **AGPL-3.0-only**. The commercial notice is
not itself a license grant; separate written terms are required. AGPL licensing
does not prohibit commercial activity. Third-party licenses and notices remain
applicable. See [LICENSING](LICENSING.md) for details.

Copyright © 2026 William Lawrence Avery.
