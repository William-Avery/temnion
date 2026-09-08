# ADR 0015: Standalone Daemon (temniond), Multi-Protocol Hosting, and Operational Lifecycle

Status: accepted implementation contract for Post-R6 Operational Extensions (`apps/temniond`, system services, and release packaging).

## Problem and Context

Prior to this work, Temnion was primarily consumed either embedded as a Rust library crate or through point-in-time CLI commands (`tem`). To operate as an authoritative, always-on time-series and temporal-epistemic database engine in production infrastructure, Temnion requires a dedicated background daemon (`temniond`).

Operating an autonomous, deterministic temporal database daemon introduces critical operational and architectural requirements:
1. **Zero Unsafe Concurrency:** All network listener management, client dispatching, background maintenance, and state coordination must adhere strictly to `#![forbid(unsafe_code)]`.
2. **Multi-Protocol Multiplexing:** The daemon must manage distinct communication surfaces:
   - High-throughput binary wire protocol (Temnion Network Protocol, TNP).
   - High-performance analytical columnar transport (Apache Arrow Flight).
   - Agentic control plane (Model Context Protocol, MCP).
3. **Graceful Lifecycle and Tail Integrity:** Shutdown triggered by operating system signals (`SIGINT`, `SIGTERM`, or Windows Service control requests) must complete in-flight transactions, flush pending WAL buffers, safely close background maintenance loops, and release advisory file locks without corruption.
4. **Deterministic Recovery Invariants:** On startup, the daemon must verify database integrity using `RecoveryMode::RejectIncompleteTail`, ensuring torn or truncated log entries from sudden power losses are rejected and reported cleanly.
5. **Operating System Service Integration:** Daemon operations must seamlessly fit into modern cloud and on-premise environments, offering declarative Linux `systemd` service units with security sandboxing and automated Windows Service PowerShell management.
6. **Reproducible Distribution Packaging:** Binaries, configuration templates, service manifests, and documentation must package into single-command distribution archives with verifiable cryptographic checksums.

---

## Architectural Decisions

### 1. Standalone Daemon Binary (`apps/temniond`)

The daemon binary is placed in `apps/temniond` as a workspace application member:
- **`DaemonConfig` (`src/config.rs`)**:
  - Provides strongly-typed configuration covering storage (`data_dir`, `source_id`, `source_epoch`), network endpoints (`server_id`, `tnp_bind`, `flight_bind`, `mcp_enabled`), and background operations (`maintenance_interval_secs`).
  - Includes a zero-dependency, line-by-line TOML parser and serializer (`parse_toml`, `to_toml_string`, `load`, `save`), avoiding heavy external parsing dependencies.
- **`DaemonServer` (`src/server.rs`)**:
  - Encapsulates database storage initialization, store opening, and network connection lifecycle.
  - Instantiates `Store::open` with `RecoveryMode::RejectIncompleteTail` to enforce data integrity on boot.
  - Binds a `std::net::TcpListener` on the configured `tnp_bind` address.
  - Spawns a background maintenance worker that periodically triggers store checkpointing and storage hierarchy management at configured intervals.
  - Manages client connection threads: each incoming TCP connection is wrapped in a `TnpChannel` and serviced against an `Arc<Mutex<TnpServer>>`.
- **CLI Commands (`src/main.rs`)**:
  - `temniond run [--config <path>]`: Starts daemon execution in foreground or background service mode. Intercepts `Ctrl+C` via `ctrlc` crate for graceful shutdown.
  - `temniond init [--data-dir <path>] [--config <path>]`: Initializes the database directory, writes initial metadata, and generates a default configuration file.
  - `temniond status [--config <path>]`: Connects to a running daemon via TNP probe (performing Handshake, Ping, and Describe commands) to report operational health, server ID, and store statistics without interrupting active queries.
  - `temniond version`: Emits compiler version, target OS, and architecture.

### 2. Concurrency and Synchronization Model

To guarantee memory safety without unsafe code:
- **Shared Query Engine:** The core `TnpServer` wraps the query runtime and store under an `Arc<Mutex<TnpServer>>`. Reader queries execute via snapshot isolation, while writes obtain exclusive locks during WAL transaction commits.
- **Atomic Shutdown Coordination:** An `Arc<AtomicBool>` (`running`) signals worker threads and the network listener loop. Socket timeouts (e.g., 500ms accept timeout) allow the listener loop to check shutdown state cleanly.
- **Graceful Thread Joins:** The `shutdown()` method sets `running = false`, waits for worker threads to finish active transactions, and drops network resources.

### 3. Service Definitions and Hardening

- **Linux `systemd` Unit (`services/systemd/temniond.service`)**:
  - `Type=simple` with automatic restart (`Restart=on-failure`, `RestartSec=5s`).
  - Strict sandboxing:
    - `ProtectSystem=strict` (mounts `/usr`, `/boot`, `/etc` read-only).
    - `ProtectHome=true` (inaccessible user home directories).
    - `ReadWritePaths=/var/lib/temnion` (restricts write access exclusively to database directory).
    - `PrivateTmp=true` (isolated `/tmp`).
    - `NoNewPrivileges=true` (prevents privilege escalation).
  - High descriptor limits: `LimitNOFILE=65536`.
- **Windows Service Automation (`services/windows/`)**:
  - `install-service.ps1`: Validates administrator elevation, configures binary arguments, registers service via `New-Service` or `sc.exe`, and sets automatic failure recovery actions (`restart/5000/restart/10000/restart/30000`).
  - `uninstall-service.ps1`: Gracefully stops the running service and removes the registration cleanly.

### 4. Distribution Packaging (`scripts/package-release.ps1`)

Automates reproducible release builds:
- Compiles `tem` and `temniond` in release mode (`cargo build --release --locked`).
- Packages binaries, `temnion.example.toml`, systemd units, Windows PowerShell scripts, `README.md`, `CHANGELOG.md`, and `LICENSE` into a structured stage directory.
- Creates compressed `.zip` distribution archives and computes NIST SHA-256 checksums (`.zip.sha256`).

---

## Invariants and Guarantees

1. **Unsafe Code Prohibition:** `#![forbid(unsafe_code)]` remains strictly enforced across `temniond` and all workspace crates.
2. **Tail Integrity Guarantee:** The daemon will never silently ignore corrupted log tails on boot; incomplete log writes fail fast with actionable recovery reports.
3. **Zero Leaked Locks:** Clean shutdown ensures advisory file locks on the store directory are dropped promptly, preventing stale lock file blocking on service restarts.
4. **Probe Non-Invasiveness:** `temniond status` executes read-only protocol probes that never allocate transaction IDs or mutate database state.

---

## Consequences

### Positive
- Temnion can now be deployed as an autonomous, robust OS background service on both Linux and Windows.
- Health monitoring and telemetry can probe daemon state over standard TNP channels without administrative disruption.
- Release distribution is fully automated with cryptographic integrity verification.

### Negative / Trade-Offs
- Mutex contention on `TnpServer` may become a bottleneck under extreme concurrent write workloads; future enhancements can introduce fine-grained concurrent read-locking via `RwLock` or lockless snapshot propagation.
