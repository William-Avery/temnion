# Temnion Studio

Temnion Studio is an **initial, experimental M23 desktop slice** built with
Tauri 2, Rust, React, TypeScript, TanStack Query and TanStack Table. It is a
native client for bounded operations against one local Temnion database; it is
not a second database engine and it does not start a network listener.

## Current capability

The native host currently provides real flows for:

- creating or opening one local durable database;
- executing TemQL, compact `tn:` and SQL scans through the canonical query IR;
- editing queries with GitHub-style syntax highlighting, line numbers, active-line
  focus and language-aware token colors for TemQL, compact Tem and SQL;
- displaying bounded results through TanStack Table;
- producing `EXPLAIN` output without opening a database;
- reading bounded durable history and plotting valid time against known time;
- appending one validated event with an OS-synchronized durability receipt;
- reading branch-manifest metadata; and
- tracing declared causal ancestry and effects from bounded durable history.

The webview never receives a raw `Store`, filesystem capability or unbounded
history. The Rust host owns the database path, writer lock, recovery validation
and command budgets. Connecting uses `RejectIncompleteTail`; recovery remains an
explicit CLI operation.

## Prerequisites

- Rust 1.85.0 (the repository toolchain)
- Node.js 22 and npm
- Windows x64: Microsoft Edge WebView2 and Visual Studio C++ Build Tools
- Linux desktop: the Tauri 2 WebKitGTK 4.1 system prerequisites for the target
  distribution

Only the Windows x64 native host is checked in the current Studio CI job. Linux
x64 packaging and native Linux ARM64/JetPack 6.2.2 desktop execution are not yet
qualified and must not be inferred from the core Rust cross-check.

## Build and run

From the repository root:

```text
cd apps/temnion-studio
npm ci
npm run build
npm run tauri dev
```

`npm run dev` starts a browser-only frontend preview. It is intentionally
disconnected because local database access exists only in the native Tauri host.

Run the native checks directly with:

```text
cargo fmt --manifest-path apps/temnion-studio/src-tauri/Cargo.toml -- --check
cargo clippy --locked --manifest-path apps/temnion-studio/src-tauri/Cargo.toml --all-targets --target-dir target/temnion-studio -- -D warnings
cargo test --locked --manifest-path apps/temnion-studio/src-tauri/Cargo.toml --target-dir target/temnion-studio
```

Build platform packages with `npm run tauri build`. A successful compile or
cross-check is not a native-platform qualification report.

## First local flow

1. Open **Connections**.
2. Enter a database directory. Choose **Create new** for a new path or **Open
   existing** for a directory that already contains `events.wal`.
3. Use **Ingestion** to append a small hexadecimal payload. Entity IDs use
   `shard:slot:generation`; time values use `clock:tick`; causal IDs use
   `source:epoch:sequence`.
4. Use **Query Studio** with one of the supplied TemQL, compact Tem or SQL
   examples.
5. Inspect the same events in **Temporal Plane**, **Branches & Causality**, and
   **Storage & Capabilities**.

Opening the same database concurrently in the CLI or another Studio process can
fail because the durable store intentionally holds a single-writer directory
lock.

## Enforced view limits

| Surface | Current ceiling |
| --- | ---: |
| Rows returned to a query/history view | 1,000 |
| Events scanned per command | 65,536 |
| Bytes read per command | 16 MiB |
| Event payload accepted by ingestion | 1 MiB |
| Causal traversal depth | 32 |

These are Studio command limits, not general promises about engine capacity.
Continuation UI is future work; a truncated result is labeled rather than
silently presented as complete.

## Known gaps before full M23

- persistent schema registry and field-mapping ingestion wizard;
- continuation controls, subscriptions and prepared-query caching;
- branch creation/promotion controls;
- 2D/3D Morton and richer causal graph rendering;
- Arrow export from the result grid;
- signed native installers and release/update channels;
- Linux x64 and native Linux ARM64 packaging/execution evidence; and
- end-to-end desktop automation beyond the native store round-trip test.

Local query bookmarks use webview local storage and contain query text only; no
database credentials or event payloads are written there.
