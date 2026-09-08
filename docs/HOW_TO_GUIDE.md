# The Official Temnion How-To Guide

> **Welcome to the authoritative developer guide for Temnion.**  
> Similar to Microsoft Learn for C# or React.dev for React, this guide takes you from your first installation through daemon management, connection strings, CLI workflows, Studio Workbench exploration, SQL/TemQL query authoring, standalone cognitive clients, and production storage operations.

---

## Table of Contents

1. [Introduction & Core Mental Model](#1-introduction--core-mental-model)
2. [Installation & Component Setup](#2-installation--component-setup)
3. [Running the Database Server (`temniond`)](#3-running-the-database-server-temniond)
4. [Connecting to Temnion](#4-connecting-to-temnion)
5. [Using the Command-Line Interface (`tem`)](#5-using-the-command-line-interface-tem)
6. [Temnion Studio Workbench IDE Guide](#6-temnion-studio-workbench-ide-guide)
7. [Language Reference: SQL & TemQL](#7-language-reference-sql--temql)
8. [Standalone Cognitive Client (`tzeentch`)](#8-standalone-cognitive-client-tzeentch)
9. [Durability, Storage & Production Maintenance](#9-durability-storage--production-maintenance)

---

## 1. Introduction & Core Mental Model

### What is Temnion?
Temnion is an open-source, high-performance, bitemporal event-sourcing database engine engineered entirely in safe Rust under `#![forbid(unsafe_code)]`. Unlike conventional relational databases that overwrite row state or single-timeline event stores, Temnion is founded upon three core primitives:

```
+-------------------------------------------------------------------------------+
|                                Temnion Core                                   |
+-------------------------------------------------------------------------------+
| 1. Multi-Clock Coordinate System:                                             |
|    - Physical Time: Monotonic wall-clock nanoseconds (validity window)        |
|    - Logical Time: Lamport / causal sequence counters (total commit ordering) |
|    - Causal Horizon: Cryptographic parent hash vector DAG                     |
|                                                                               |
| 2. Timeline Branching:                                                        |
|    - First-class zero-copy forks for speculative planning, simulations,       |
|      and counterfactual reasoning                                             |
|                                                                               |
| 3. Storage Architecture:                                                      |
|    - Active Segment: Crash-resilient Write-Ahead Log (WAL)                     |
|    - Immutable Segments: Compressed Columnar TSF files guarded by             |
|      SegmentManifest (`segments/manifest.bin`)                                |
|    - Reference Holds: Guaranteed retention locks preventing compaction        |
+-------------------------------------------------------------------------------+
```

### Key Differences from Traditional Databases
| Feature | Relational DB (PostgreSQL / MySQL) | Key-Value / Doc (Redis / Mongo) | Temnion |
| :--- | :--- | :--- | :--- |
| **History & Time Travel** | None (overwrites unless auditing tables) | None or snapshot-only | **Native Multi-Clock Bitemporal** |
| **Branching** | Physical dump / restore | None | **Zero-Copy Instant Forks** |
| **Memory Safety** | C / C++ (memory vulnerability risks) | C / C++ / Go | **100% Safe Rust (`#![forbid(unsafe_code)]`)** |
| **Wire Protocol** | Postgres Wire / MySQL Protocol | RESP / Custom | **TNP (TCP 9180) & Arrow Flight SQL (9181)** |
| **Durability** | In-place pages + WAL | Append-only file / snapshot | **Manifest-guarded TSF segments & WAL retirement** |

---

## 2. Installation & Component Setup

Temnion ships with a PostgreSQL-style component installer allowing you to pick and choose exactly the components you need:
- **`tem`**: Command-line client for query and inspection.
- **`temniond`**: Always-on background server daemon.
- **`temnion-studio`**: Database administration IDE & Workbench.
- **`tzeentch`**: Standalone cognitive consumer client.
- **Service Integration**: Windows Service or Linux `systemd` daemon.

### Option A: Windows Automated Setup (PowerShell)

Run the interactive setup wizard in Windows PowerShell or PowerShell 7 (`pwsh`):

```powershell
# Interactive Component Wizard
pwsh installer/install.ps1
```

The wizard prompts for component checkboxes, listening port, database name, and admin credentials:
```text
================================================================
           Temnion Database Component Setup Wizard
================================================================
Select components to install:
  [X] 1. Temnion CLI (tem)
  [X] 2. Temnion Daemon Server (temniond)
  [X] 3. Temnion Studio Workbench (temnion-studio)
  [X] 4. Tzeentch Autonomous Client (tzeentch)
  [X] 5. Windows Service Registration
================================================================
Enter TCP Listening Port for TNP [9180]: 9180
Enter Arrow Flight SQL Port [9181]: 9181
Enter Database Name [temnion]: temnion
Enter Superuser Username [admin]: admin
Enter Superuser Password: ********
Confirm Superuser Password: ********
[OK] Installation completed successfully!
```

#### Unattended / Headless CI/CD Installation
For Docker, CI/CD runners, or infrastructure automation, use the `-Silent` flag:
```powershell
pwsh installer/install.ps1 `
  -Silent `
  -InstallDir "C:\Program Files\Temnion" `
  -DataDir "C:\ProgramData\Temnion\data" `
  -Port 9180 `
  -FlightPort 9181 `
  -DatabaseName "production" `
  -AdminUser "admin" `
  -AdminPassword "Secr3tToken!"
```

---

### Option B: Linux & macOS Setup (POSIX Shell)

Run the Unix installer script:
```bash
# Interactive setup
chmod +x installer/install.sh
./installer/install.sh

# Unattended / Silent setup
./installer/install.sh \
  --silent \
  --prefix /usr/local \
  --data-dir /var/lib/temnion \
  --port 9180 \
  --flight-port 9181 \
  --database temnion \
  --user admin \
  --password "Secr3tToken!"
```

---

### Option C: Building from Source (Cargo)

Prerequisites: Rust toolchain 1.85+ (`rustup default stable`).

```bash
# Clone the repository
git clone https://github.com/William-Avery/temnion.git
cd temnion

# Build all release binaries
cargo build --release --locked

# Binaries generated:
# target/release/tem          (CLI)
# target/release/temniond     (Daemon Server)
# target/release/tzeentch     (Standalone Client)
```

To run the web IDE locally:
```bash
cd apps/temnion-studio
npm install
npm run dev
# Studio IDE will be running at http://localhost:5173/
```

---

## 3. Running the Database Server (`temniond`)

The `temniond` binary manages storage, memory safety invariants, WAL retirement, background maintenance, and network listener threads.

### Step 1: Initialize Database Files & Config
```bash
# Initializes storage directory and default temnion.toml
temniond init --data-dir ./data/temnion_db --config ./temnion.toml
```

### Step 2: Configuration Reference (`temnion.toml`)
```toml
# Temnion Server Configuration
database_name = "temnion"
admin_user = "admin"
auth_token = "s3cr3t_auth_t0ken"

[storage]
data_dir = "./data/temnion_db"
max_wal_bytes = 67108864 # 64 MiB WAL ceiling before automatic segment seal
flush_interval_ms = 1000  # Durable fsync interval

[network]
tnp_bind = "127.0.0.1:9180"    # Temnion Network Protocol
flight_bind = "127.0.0.1:9181" # Apache Arrow Flight SQL

[maintenance]
interval_secs = 60             # Periodic checkpoint and WAL retirement sweep
```

### Step 3: Run the Server in Foreground
```bash
temniond run --config ./temnion.toml
```
Output:
```text
2026-09-08T05:00:00Z [INFO] Temnion daemon booting (version 0.1.0, #![forbid(unsafe_code)])
2026-09-08T05:00:00Z [INFO] Storage engine opened at ./data/temnion_db
2026-09-08T05:00:00Z [INFO] TNP listener active on 127.0.0.1:9180
2026-09-08T05:00:00Z [INFO] Arrow Flight listener active on 127.0.0.1:9181
2026-09-08T05:00:00Z [INFO] Daemon running. Press Ctrl+C to stop.
```

### Step 4: Health & Status Probing
You can query a running daemon non-disruptively over the network:
```bash
temniond status --config ./temnion.toml
```
Output:
```text
========================================
Temnion Daemon Status
========================================
Target:          127.0.0.1:9180
Status:          ONLINE
Database ID:     temnion-db-001
Active Epoch:    1
Durable Events:  14,820
Active WAL:      3.2 MiB
Retired TSFs:    4 segments
Round-Trip:      1.4 ms
Memory Safety:   forbid(unsafe_code)
========================================
```

### Step 5: Managing as a Background System Service

#### Windows Service:
```powershell
# Register and start service
pwsh services/windows/install-service.ps1

# Check Windows service status
Get-Service -Name "TemnionServer"

# Stop or restart
Restart-Service -Name "TemnionServer"
```

#### Linux systemd:
```bash
sudo cp services/systemd/temniond.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now temniond
sudo systemctl status temniond
```

---

## 4. Connecting to Temnion

Temnion supports connection strings, configuration profiles, and environment variables across all clients (`tem`, `temnion-studio`, and `tzeentch`).

### Connection URIs
```text
temnion://[user[:password]@]host[:port]/database
```
Examples:
- Default local: `temnion://127.0.0.1:9180/temnion`
- Authenticated: `temnion://admin:s3cr3t@localhost:9180/production`
- Custom cluster: `temnion://writer:token@db.internal:9180/telemetry_db`

### Profiles File (`~/.temnion/connections.toml`)
Store connection profiles for easy switching:
```toml
[default]
host = "127.0.0.1"
port = 9180
flight_port = 9181
database = "temnion"
username = "admin"
auth_token = "s3cr3t_auth_t0ken"

[production]
host = "prod-db.internal"
port = 9180
flight_port = 9181
database = "analytics_prod"
username = "admin"
auth_token = "prod_k3y_99182"
```

### Environment Variables
All clients automatically resolve missing parameters from:
- `TEMNION_HOST` (e.g. `127.0.0.1`)
- `TEMNION_PORT` (e.g. `9180`)
- `TEMNION_DATABASE` (e.g. `temnion`)
- `TEMNION_USER` (e.g. `admin`)
- `TEMNION_AUTH_TOKEN` (e.g. `token`)

---

## 5. Using the Command-Line Interface (`tem`)

The `tem` command-line utility enables rapid data exploration and scripted pipelines.

### Connecting & Probing
```bash
# Ping active connection
tem ping --uri temnion://127.0.0.1:9180/temnion

# Show database status and storage statistics
tem status
```

### Ingesting Records
```bash
# Ingest structured entity observation
tem ingest \
  --schema "sensor_telemetry" \
  --entity "sensor_42" \
  --fields '{"temperature": 24.5, "humidity": 58.2, "status": "nominal"}'
```

### Querying Data
```bash
# Execute SQL query
tem query --sql "SELECT * FROM sensor_telemetry WHERE entity_id = 'sensor_42'"

# Point-in-time time travel query
tem query --sql "SELECT * FROM sensor_telemetry AS OF SYSTEM_TIME '2026-09-08T00:00:00Z'"

# Execute native TemQL expression
tem query --temql "from sensor_telemetry where temperature > 22.0 select entity_id, temperature"
```

### Managing Branches & Timelines
```bash
# List timeline branches
tem branch list

# Fork a new speculative branch from main
tem branch create --from main --name "speculative_sim_alpha"

# Query the speculative branch
tem query --branch "speculative_sim_alpha" --sql "SELECT * FROM sensor_telemetry"
```

---

## 6. Temnion Studio Workbench IDE Guide

Temnion Studio is the official database administration and query explorer interface (equivalent to MySQL Workbench or pgAdmin).

```
+-----------------------------------------------------------------------------------+
|  TEMNION STUDIO                                  [Connected · 14,820 events]      |
+-------------------+---------------------------------------------------------------+
| LEARN & DOCS      | ⚡ Query Studio                                               |
|  📖 How-To Guide  | [TEMQL] [COMPACT] [SQL]                       Max Rows: [100] |
|                   | +-----------------------------------------------------------+ |
| WORKBENCH         | | SELECT entity_id, temperature, humidity                   | |
|  ⚡ Query Studio   | | FROM sensor_telemetry                                     | |
|  ⊞ Schema Catalog | | WHERE temperature > 23.0                                  | |
|  ◴ Temporal Plane | +-----------------------------------------------------------+ |
|  ⑂ Branches       | [ Explain ]  [ Execute Query ]                                |
|                   |                                                               |
| OPERATE           | Results (12 rows in 1.4ms)                                    |
|  ◎ Connections    | #  | entity_id | temperature | humidity | physical_time     |
|  ⇧ Ingestion      | 1  | sensor_01 | 24.2 °C     | 54%      | 2026-09-08T04:12Z |
|  ▥ Storage & Cap. | 2  | sensor_02 | 23.8 °C     | 57%      | 2026-09-08T04:14Z |
+-------------------+---------------------------------------------------------------+
```

### Key Workspaces

#### 1. 📖 How-To Guide
- Integrated interactive documentation viewer containing this complete guide.
- Live code snippet copy buttons with immediate visual feedback.
- Quick-action buttons (`"Try in Query Studio"`) that pre-load real queries into the editor.

#### 2. ⚡ Query Studio
- Multi-syntax support: **SQL**, **TemQL**, and **Compact (`tn:`)**.
- Interactive query execution with bounded result streaming.
- Query plan analysis with the **Explain** button (displays cost, scan pruning, and E-Graph rewrite passes).
- Exportable and bookmarkable query history.

#### 3. ⊞ Schema & Entity Catalog
- Inspect declared schemas, column types (`I64`, `F64`, `Utf8`, `Bytes`, `Timestamp`).
- View indexing statistics: **Bloom filters** for point lookups and **ZoneMaps** (Min/Max values) for physical sequence pruning.
- Inspect active entity slots and monotonically advancing revision counters.

#### 4. ◴ Temporal Plane
- Interactive multi-clock time travel scrubber.
- Dual-axis sliders for **Physical Monotonic Time** and **Logical Sequence Ordering**.
- Observe entity state transitions exactly as they existed at any nanosecond in history.

#### 5. ⑂ Branches & Causality
- Visual DAG of timeline branches.
- Inspect parent commits, cryptographic hashes, and merge causality.
- Create new branches for speculative simulation without duplicating storage.

#### 6. ◎ Connections Manager
- Manage saved connection profiles (`Local Dev`, `Staging`, `Production`).
- Test connection latency, server identification, and advertised capabilities with one click.
- Switch active database instances seamlessly.

---

## 7. Language Reference: SQL & TemQL

Temnion supports both standard SQL-92 dialects and native TemQL expressions, which lower to a single canonical typed Intermediate Representation (IR).

### Temporal Predicates in SQL

#### Point-in-Time Flashback (`AS OF SYSTEM_TIME`)
Retrieves the authoritative state of the entity at a precise physical timestamp:
```sql
SELECT entity_id, temperature, status
FROM sensor_telemetry
AS OF SYSTEM_TIME '2026-09-08T03:30:00.000Z'
WHERE entity_id = 'sensor_42';
```

#### Physical Time Interval Scanning
Scans mutations that were physically committed within a nanosecond time boundary:
```sql
SELECT entity_id, temperature, humidity
FROM sensor_telemetry
BETWEEN PHYSICAL 1757300000000000000 AND 1757303600000000000
ORDER BY physical_time DESC;
```

#### Causal DAG Horizon Scans
Constrains results to records that fall within a specific cryptographic ancestry:
```sql
SELECT *
FROM action_decision
WITHIN CAUSAL HORIZON 'b3a8...91f2'
LIMIT 50;
```

---

### TemQL Native Syntax
TemQL is a concise, composable query language optimized for temporal event flows:

```text
from sensor_telemetry
  where temperature >= 25.0 and status == "nominal"
  as_of physical 1757301000000000000
  select entity_id, temperature, humidity
  order by temperature desc
  limit 25
```

---

## 8. Standalone Cognitive Client (`tzeentch`)

`tzeentch` is an autonomous cognitive consumer client that lives completely separate from the core database and Temnion Studio. It consumes temporal event streams, performs real-time cadence tracking, and emits speculative decision events over TNP.

```
+-------------------------------------------------------+
|                    Temnion Engine                     |
|            (TNP: 9180 | Arrow Flight: 9181)           |
+---------------------------+---------------------------+
                            |
             +--------------+--------------+
             |                             |
             v                             v
+--------------------------+ +--------------------------+
|      Temnion Studio      | |         Tzeentch         |
|      (Workbench IDE)     | |   (Autonomous Client)    |
| - Connections Manager    | | - Cadence Stream Tracker |
| - Schema Catalog         | | - Decision Trace Probe   |
| - Query Runner           | | - Speculative Planner    |
+--------------------------+ +--------------------------+
```

### Running the Tzeentch CLI
```bash
# Check daemon connectivity from Tzeentch
tzeentch ping --host 127.0.0.1 --port 9180

# Query engine status and cognitive consumer state
tzeentch status

# Measure event ingestion cadence & latency metrics
tzeentch cadence --interval 5

# Trace causal history for a specific sequence
tzeentch trace 42
```

---

## 9. Durability, Storage & Production Maintenance

Temnion guarantees durability and high-throughput write performance via a tiered architecture.

```
Incoming Writes
      │
      ▼
┌──────────────────────────────────────────────┐
│  Write-Ahead Log (WAL)                       │
│  - CRC32 verification                        │
│  - Torn write detection (RejectIncomplete)   │
│  - Memory-mapped append buffer               │
└──────────────────────┬───────────────────────┘
                       │
             Automatic / Manual Seal
                       │
                       ▼
┌──────────────────────────────────────────────┐
│  SegmentManifest (segments/manifest.bin)     │
│  - Registers published .tsf segments         │
│  - Sequence intervals & byte sizes           │
│  - Retired sequence watermarks               │
└──────────────────────┬───────────────────────┘
                       │
          Store::retire_wal Compaction
                       │
                       ▼
┌──────────────────────────────────────────────┐
│  Immutable TSF Columnar Segments             │
│  - Compression & Dictionary Encoding         │
│  - ZoneMaps & Bloom Filters                  │
│  - ReferenceHold Protection                  │
└──────────────────────────────────────────────┘
```

### WAL Retirement & Compaction
When the WAL reaches its byte threshold (e.g., 64 MiB), the engine seals active frames into immutable `.tsf` segment files and records them in `SegmentManifest`. Calling `Store::retire_wal` safely truncates the retired WAL prefix while strictly honoring any active `ReferenceHold` locks (e.g., active long-running queries or backups).

### Creating and Verifying Backups
```bash
# Create physical consistent backup
tem backup create --dest /var/backups/temnion/snapshot_001

# Verify backup manifest and segment CRCs
tem backup verify --src /var/backups/temnion/snapshot_001

# Restore into a fresh data directory
tem backup restore --src /var/backups/temnion/snapshot_001 --dest /var/lib/temnion/data
```

---

## Summary & Next Steps

You now have a complete, production-grade understanding of Temnion.
- To explore queries immediately, open **Temnion Studio** and navigate to **⚡ Query Studio**.
- To review or add connections, visit **◎ Connections Manager**.
- For engine metrics, inspect **▥ Storage & Capabilities**.
