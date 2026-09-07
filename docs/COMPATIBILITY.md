# Compatibility and platform policy

## Current foundation

The development workspace exposes Rust library APIs, a volatile demo and
durable source-log CLI commands. WAL/batch and TSF v1 layouts are explicitly
encoded and documented in [BINARY_FORMAT](BINARY_FORMAT.md); scalar schema/
mutation encoding has its own v1 contract in [SCHEMA](SCHEMA.md). Unknown
versions fail explicitly. There is no TNP protocol, C ABI, TemQL parser or
production service. Rust memory layout is not an interchange encoding.

- **MSRV:** Rust 1.85; reproducible baseline pinned to **1.85.0**.
- **Workspace:** edition 2024, resolver 3, committed `Cargo.lock`.
- **Dependency boundary:** core/state/events/schema use std and local crates;
  storage adds explicit checksum/locking/entropy dependencies. Project source
  forbids unsafe code.
- **CI configuration:** native Windows x64/MSVC and Linux x64/GNU formatting,
  Clippy and tests, with pinned and latest-stable Rust checks.
- **ARM64:** `cargo check` for `aarch64-unknown-linux-gnu` compiles/type-checks code
  without linking or executing ARM64 binaries. It does not qualify native tests,
  performance, filesystem behavior, glibc compatibility or Studio.
- **Native ARM64 goal:** AGX Orin Developer Kit, JetPack 6.2.2 / L4T 36.5 /
  Ubuntu 22.04. Native execution and eventual Tauri/WebKitGTK desktop qualification
  remain future work. Release binaries must match the supported userspace baseline.

Each increment must have its actual hosted CI result associated with its commit.
Latest stable is an additional compatibility signal, not an implicit MSRV bump.
MSRV/edition/target changes require an explicit review and release note.

Rust APIs are pre-1.0: do not assume source compatibility across 0.x minor
versions. Keep patch releases source-compatible except documented necessary
correctness/security fixes. Document breaking changes and migration examples in
the same change; do not silently reinterpret identity, time or receipt semantics.

Current `tem describe` JSON reports only implemented capabilities. Consumers
must check capabilities rather than infer support from names in the architecture.
Its foundation shape is not a frozen TNP negotiation schema. Unknown CLI commands
and malformed arguments must remain errors with nonzero exit status.

## Policy before new persistent or external contracts ship

These remain requirements as T01/T04–T06/T11+ expand the current v1 formats:

1. Independently version TSF, WAL, manifests/catalogs, TNP, query/schema IR, model
   manifests and the C ABI. Define readers/writers, required/optional features,
   and explicit compatibility matrices.
2. Specify byte order, field widths, lengths, checksums, bounds, unknown fields,
   reserved values and version rejection. Never serialize raw Rust struct memory.
3. Unknown required features, unsupported operators/precision, or incompatible
   schema versions return typed errors. No success-shaped fallback.
4. Bind prepared queries/cursors/subscriptions to schemas, snapshots, branches,
   ordering, authorization and expiry. Changes cannot silently move their
   logical positions or weaken exactness.
5. Golden fixtures and cross-version tests must accompany a claimed stable
   contract. Include malformed/truncated data, resource limits, upgrades, and
   downgrade behavior only where explicitly supported.
6. Migration preserves original evidence and uses validated, recoverable
   publication. Test interrupted migration and retained old readers. Back up and
   verify restore before destructive lifecycle changes.
7. C/Arrow interfaces specify ownership, lifetime, release, threading, panic
   containment and typed failure. A shared-memory descriptor needs additional
   bounds/synchronization/crash-recovery rules.
8. Release notes state compatibility windows, deprecations, migration/rollback
   paths and unsupported combinations. Do not promise backward compatibility
   before fixtures and recovery tests establish it.

Remote exposure remains opt-in with explicit access controls. A protocol version
does not replace authentication, TLS or authorization. The same semantic/policy
contracts apply to Rust, TemQL, Tem, MCP, optional SQL, IPC and Studio.

## Evidence required for release claims

Record source revision, lockfile/toolchain, target triple, actual native hardware,
OS/userspace and commands/results. Publish only platforms exercised at the
claimed level: check, link, native test, native benchmark, installer or desktop.
Never collapse those levels into a generic “supported” badge.

Gate production persistence on fault-injected recovery and compatibility tests.
Gate performance statements on [BENCHMARKING](BENCHMARKING.md).
Gate EKS/evolution on [Gates A/B/C](ROADMAP.md#gates). No gate is passed by the
initial foundation.
