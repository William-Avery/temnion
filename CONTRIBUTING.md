# Contributing to Temnion

## Outside code contributions are paused

Do not submit outside code for acceptance or merge until William Lawrence Avery
has approved contribution terms preserving both the AGPL and commercial
licensing paths. This includes copied patches, code offered through issues, and
third-party code incorporated into owner-authored changes.

A Developer Certificate of Origin (DCO), sign-off, public fork, or ordinary
pull-request submission is **not** a substitute for an approved CLA, copyright
assignment, or other adequate relicensing agreement. No such agreement is
established by this document. This acceptance policy does not restrict rights
already granted by the AGPL.

Bug reports, documentation corrections described in words, benchmark methodology
feedback, and design discussion are welcome. Discuss intended changes before
preparing a contribution. For licensing questions contact
**willaveryvy@gmail.com**; report vulnerabilities through [SECURITY](SECURITY.md).

## Owner-authorized development

1. Read the [architecture](docs/ARCHITECTURE.md),
   [roadmap](docs/ROADMAP.md), and [foundation contracts](docs/adr/0001-foundation-contracts.md).
2. Keep a change scoped to one implemented capability or explicit design decision.
   Preserve source milestone traceability and update relevant docs together.
3. Add targeted tests for changed behavior and failure modes. Use deterministic,
   minimal, rights-cleared fixtures; do not add private recordings, credentials,
   model weights, or large generated benchmark output.
4. Keep the headless default dependency graph small. No consumer internals,
   model/GUI runtimes, or networking in mandatory storage paths. Project source
   forbids unsafe code; changing that requires a separate reviewed decision.
5. Run the existing checks from the repository root:

   ```text
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   cargo run -p temnion-cli -- demo
   ```

6. Use the pinned Rust 1.85.0 baseline. CI additionally tests latest stable on
   Windows/Linux x64 and cross-checks Linux ARM64. Record which tests actually
   ran; never label a cross-check as native qualification.
7. Document API, schema, format, budget, and durability effects. Preserve the
   distinction between volatile admission and future durable acknowledgment.
8. Follow [BENCHMARKING](docs/BENCHMARKING.md) for performance-sensitive changes.
   Publish reproducible evidence, not inferred throughput or target numbers.

Keep `Cargo.lock` under version control. Review every dependency's origin,
license, target support, and transitive graph before updating
[the inventory](docs/DEPENDENCIES.md). Preserve all applicable notices.

## Review boundaries

- Public project-owned source uses `SPDX-License-Identifier: AGPL-3.0-only`.
- The commercial notice does not override third-party licensing.
- Do not copy Tzeentch code into Temnion. Its declared MIT/Apache licensing is
  separate, and future integration requires its own rights and compatibility review.
- Every advertised feature needs a working implementation, relevant tests,
  documentation, and a runnable example. A placeholder crate or mocked success
  path is not feature completion.
- CI changes must pin actions to reviewed full commit SHAs. Do not introduce
  secrets, write tokens, privileged PR workflows, or untrusted code execution on
  self-hosted hardware.
- Preserve the two architecture source artifacts unchanged. Maintain current
  decisions in the architecture, roadmap, and ADRs instead.
