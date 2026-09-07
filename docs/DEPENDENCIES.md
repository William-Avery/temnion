# Dependency and licensing inventory

## Public project code

The workspace contains `temnion-core`, `temnion-state`, `temnion-events`,
`temnion-format`, `temnion-schema`, `temnion-storage`, `temnion-bench`, and
`temnion-cli`. Core/state/events/schema remain std-only. Persistent formats and
storage use the following locked crates.io dependencies.

| Package | Resolved version | Purpose | Declared license |
| --- | --- | --- | --- |
| `crc32fast` | 1.5.1 | WAL/TSF corruption-detection checksums, not cryptographic authentication | MIT OR Apache-2.0 |
| `fs2` | 0.4.3 | Cross-platform OS file locks for one writer | MIT/Apache-2.0 |
| `getrandom` | 0.2.17 | OS entropy for persistent database and temporary-file identities | MIT OR Apache-2.0 |
| `cfg-if` | 1.0.4 | Target-dependent dependency implementation | MIT OR Apache-2.0 |
| `libc` | 0.2.189 | Unix system interfaces used by the dependencies | MIT OR Apache-2.0 |
| `wasi` | 0.11.1+wasi-snapshot-preview1 | Target-specific entropy interface; not a qualified Temnion runtime target | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| `winapi` | 0.3.9 | Windows system interfaces used by the dependencies | MIT/Apache-2.0 |
| `winapi-i686-pc-windows-gnu`, `winapi-x86_64-pc-windows-gnu` | 0.4.0 | Target-specific import support, not additional platform qualification | MIT/Apache-2.0 |

Versions/licenses above come from the resolved Cargo metadata and lockfile.
Low-level dependency implementations may use unsafe code; the workspace's
`unsafe_code = "forbid"` applies to project-owned crates, not upstream internals.
Keep upstream notices/licenses when distributing binaries or vendored sources.

Project-owned public source is **AGPL-3.0-only**, copyright © 2026 William Lawrence
Avery. A separate written commercial agreement may grant other rights;
[COMMERCIAL_LICENSE.md](../COMMERCIAL_LICENSE.md) alone grants none. The three
approved licensing templates are retained with project/holder/contact substitutions.
See [LICENSING.md](../LICENSING.md).

The root [LICENSE](../LICENSE) is the complete AGPL-3.0-only text retrieved
verbatim from the [SPDX license-list-data source](https://github.com/spdx/license-list-data/blob/3ac5a9c241d97f95b22a5e366c9c841404a35639/text/AGPL-3.0-only.txt).

SHA-256: `d8a6cc31abc16b6748c7a21f21611f5a1ec33f67d22ca23d7da1c19b95496bee`.

Do not add a project copyright header inside the canonical FSF license text.

## Build and CI tools

| Item | Role / boundary | License and update consideration |
| --- | --- | --- |
| Rust 1.85.0, Cargo, rustfmt, Clippy, standard library | Pinned headless toolchain; latest stable is an extra CI check | Rust components carry their own MIT/Apache-2.0 and component-specific third-party notices; use upstream distributions and preserve required notices when redistributing |
| `actions/checkout` v7.0.1 | Hosted CI source checkout only; `3d3c42e5aac5ba805825da76410c181273ba90b1` | MIT; action dependencies retain their own notices |
| GitHub-hosted runner images | Windows x64/Linux x64 validation; Linux ARM64 cross-check only | Runner software is not Temnion source and is not relicensed by the commercial notice |

The checkout revision was resolved from the official repository's v7.0.1 tag.
The action entrypoint, inputs and changelog were inspected for this use. It is
configured with `persist-credentials: false` and read-only repository permissions.
This limited integration review is not a claim of a complete third-party
security audit. There are no publication steps, write tokens or self-hosted PR
runners. Rust installation uses hosted rustup directly rather than another action.
Future action updates must resolve and review a full commit revision.

No Node/npm, Tauri, GUI, model, MCP, Arrow, Tokio or SQL packages are application
dependencies of this foundation. The checkout action's own Node runtime is CI
infrastructure, not a GUI/application dependency.

## Before adding or redistributing dependencies

1. Inspect the resolved Cargo graph, including target, feature, build and
   transitive dependencies. Record version/source/license and why it is needed.
2. Review compatibility with the public distribution and separately negotiated
   commercial rights. Obtain owner approval where rights are unclear.
3. Preserve dependency copyright/license/NOTICE files and applicable source
   obligations. Commercial terms for Temnion do not replace third-party terms.
4. Update this inventory, lockfile and relevant target/MSRV checks with the change.
   Review bundled artifacts/system components separately when packaging releases.
5. Keep consumer-specific and optional control/model/GUI dependencies outside
   mandatory core hot paths.

Tzeentch source is not copied here. Its declared MIT/Apache licensing is separate
and does not authorize silently changing its license or omitting required
notices. Any future adapter/combined distribution needs its own rights review.
Outside code acceptance remains paused under [CONTRIBUTING](../CONTRIBUTING.md)
until owner-approved CLA/relicensing terms exist; a DCO alone is insufficient.
