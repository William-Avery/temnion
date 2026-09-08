// SPDX-License-Identifier: AGPL-3.0-only

import { useState } from "react";

export type GuideChapter =
  | "quickstart"
  | "install"
  | "daemon"
  | "connections"
  | "cli"
  | "studio"
  | "sql"
  | "tzeentch"
  | "durability";

export interface GuideProps {
  onNavigate: (view: "guide" | "query" | "schemas" | "history" | "causality" | "connections" | "ingest" | "metrics", querySnippet?: string) => void;
}

export function CodeSnippet({
  code,
  lang = "bash",
  title,
}: {
  code: string;
  lang?: string;
  title?: string;
}) {
  const [copied, setCopied] = useState(false);

  const handleCopy = () => {
    navigator.clipboard.writeText(code).catch(() => {});
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <div className="docs-code-card">
      <div className="code-header">
        <span className="code-lang-tag">{title ?? lang}</span>
        <button
          className={`copy-code-btn ${copied ? "copied" : ""}`}
          onClick={handleCopy}
          type="button"
        >
          {copied ? "✓ Copied!" : "Copy code"}
        </button>
      </div>
      <pre className="docs-code-block">
        <code>{code}</code>
      </pre>
    </div>
  );
}

export function GuidePanel({ onNavigate }: GuideProps) {
  const [chapter, setChapter] = useState<GuideChapter>("quickstart");
  const [installPlatform, setInstallPlatform] = useState<"powershell" | "bash" | "cargo">("powershell");

  const chapters: Array<{ id: GuideChapter; title: string; icon: string }> = [
    { id: "quickstart", title: "5-Minute Quickstart", icon: "🚀" },
    { id: "install", title: "Installation & Setup", icon: "💾" },
    { id: "daemon", title: "Database Daemon (temniond)", icon: "🖥️" },
    { id: "connections", title: "Connections & Networking", icon: "🔌" },
    { id: "cli", title: "Command-Line (tem)", icon: "💻" },
    { id: "studio", title: "Studio Workbench Tour", icon: "🎨" },
    { id: "sql", title: "SQL & TemQL Reference", icon: "📜" },
    { id: "tzeentch", title: "Standalone Client (tzeentch)", icon: "🤖" },
    { id: "durability", title: "Durability & Storage", icon: "🛡️" },
  ];

  return (
    <section className="view-panel active">
      <div className="panel-header">
        <div className="panel-title-group">
          <h2>Documentation & How-To Guide</h2>
          <p className="panel-subtitle">
            Comprehensive developer reference for Temnion — from setup and connection strings to bitemporal queries and storage durability.
          </p>
        </div>
        <div className="header-actions">
          <button
            className="btn btn-primary btn-sm"
            onClick={() => onNavigate("query")}
            type="button"
          >
            ⚡ Open Query Studio
          </button>
          <button
            className="btn btn-secondary btn-sm"
            onClick={() => onNavigate("connections")}
            type="button"
          >
            ◎ Connections Manager
          </button>
        </div>
      </div>

      <div className="docs-layout">
        {/* Navigation Column */}
        <aside className="docs-nav">
          <div className="docs-nav-title">Guide Chapters</div>
          {chapters.map((item) => (
            <button
              className={`docs-nav-link ${chapter === item.id ? "active" : ""}`}
              key={item.id}
              onClick={() => setChapter(item.id)}
              type="button"
            >
              <span>{item.icon}</span>
              <span>{item.title}</span>
            </button>
          ))}
        </aside>

        {/* Content Column */}
        <main className="docs-content-area">
          {/* Chapter 1: Quickstart */}
          {chapter === "quickstart" && (
            <article className="docs-chapter">
              <h2>🚀 5-Minute Quickstart</h2>
              <p className="docs-lead">
                Get up and running with Temnion in under five minutes. Learn how to install the server, start the background daemon, and execute your first bitemporal query.
              </p>

              <div className="docs-alert alert-tip">
                <div className="alert-title">Interactive Browser Demo Ready</div>
                <div>
                  Temnion Studio currently runs with an interactive in-memory engine loaded with realistic telemetry and multi-clock events. You can start executing queries right away in <strong>Query Studio</strong> without setting up local files!
                </div>
              </div>

              <h3>Step 1: Install Temnion</h3>
              <p className="docs-paragraph">
                Use the automated installer wizard to install the daemon server, CLI tool, Studio Workbench, and standalone Tzeentch client:
              </p>
              <CodeSnippet
                code={`# Run the interactive Windows PowerShell installer\npwsh installer/install.ps1\n\n# Or on Linux / macOS\nchmod +x installer/install.sh && ./installer/install.sh`}
                lang="powershell"
                title="Installer Command"
              />

              <h3>Step 2: Start the Background Daemon</h3>
              <p className="docs-paragraph">
                Initialize the database files and launch the server in the background:
              </p>
              <CodeSnippet
                code={`# 1. Initialize store and default configuration\ntemniond init --data-dir ./data/temnion_db\n\n# 2. Run the database daemon (listens on TNP 9180 and Flight SQL 9181)\ntemniond run --config ./temnion.toml`}
                lang="bash"
                title="Server Daemon"
              />

              <h3>Step 3: Run Your First Query in Studio</h3>
              <p className="docs-paragraph">
                Open Query Studio and execute a point-in-time time-travel query using standard SQL:
              </p>
              <CodeSnippet
                code={`SELECT entity, schema, valid_time, known_time, sequence\nFROM temnion\nAS OF SYSTEM_TIME '2026-09-08T00:00:00Z'\nLIMIT 50`}
                lang="sql"
                title="SQL Time Travel Query"
              />

              <div className="docs-action-bar">
                <button
                  className="btn btn-primary"
                  onClick={() =>
                    onNavigate(
                      "query",
                      "SELECT entity, schema, valid_time, known_time, sequence\nFROM temnion\nAS OF SYSTEM_TIME '2026-09-08T00:00:00Z'\nLIMIT 50"
                    )
                  }
                  type="button"
                >
                  ⚡ Try This Query in Query Studio
                </button>
                <button
                  className="btn btn-secondary"
                  onClick={() => onNavigate("connections")}
                  type="button"
                >
                  ◎ Configure Connections
                </button>
              </div>
            </article>
          )}

          {/* Chapter 2: Install */}
          {chapter === "install" && (
            <article className="docs-chapter">
              <h2>💾 Installation & Component Setup</h2>
              <p className="docs-lead">
                Temnion provides a PostgreSQL-style component installer allowing you to pick and choose exactly what to install: the CLI, the daemon, the Studio Workbench, or the standalone client.
              </p>

              <div className="platform-switcher">
                <button
                  className={`platform-btn ${installPlatform === "powershell" ? "active" : ""}`}
                  onClick={() => setInstallPlatform("powershell")}
                  type="button"
                >
                  Windows (PowerShell)
                </button>
                <button
                  className={`platform-btn ${installPlatform === "bash" ? "active" : ""}`}
                  onClick={() => setInstallPlatform("bash")}
                  type="button"
                >
                  Linux & macOS (Bash)
                </button>
                <button
                  className={`platform-btn ${installPlatform === "cargo" ? "active" : ""}`}
                  onClick={() => setInstallPlatform("cargo")}
                  type="button"
                >
                  Source (Cargo)
                </button>
              </div>

              {installPlatform === "powershell" && (
                <>
                  <h3>Windows Interactive Setup</h3>
                  <p className="docs-paragraph">
                    Run the setup script in Windows PowerShell 5.1 or PowerShell 7 (`pwsh`). It presents numbered checkboxes to toggle components, prompts for database port (default 9180), database name, and superuser password:
                  </p>
                  <CodeSnippet
                    code={`pwsh installer/install.ps1`}
                    lang="powershell"
                    title="Windows Interactive Wizard"
                  />

                  <h3>Unattended / Silent Automation</h3>
                  <p className="docs-paragraph">
                    For automated deployments, Docker containers, or CI/CD runners, run in silent mode:
                  </p>
                  <CodeSnippet
                    code={`pwsh installer/install.ps1 \`\n  -Silent \`\n  -InstallDir "C:\\Program Files\\Temnion" \`\n  -Port 9180 \`\n  -FlightPort 9181 \`\n  -DatabaseName "temnion" \`\n  -AdminUser "admin" \`\n  -AdminPassword "Secr3tP@ssword!"`}
                    lang="powershell"
                    title="Silent PowerShell Setup"
                  />
                </>
              )}

              {installPlatform === "bash" && (
                <>
                  <h3>Linux & macOS Setup Script</h3>
                  <p className="docs-paragraph">
                    Run the interactive POSIX shell script:
                  </p>
                  <CodeSnippet
                    code={`chmod +x installer/install.sh\n./installer/install.sh`}
                    lang="bash"
                    title="POSIX Interactive Wizard"
                  />

                  <h3>Unattended Unix Setup</h3>
                  <CodeSnippet
                    code={`./installer/install.sh \\\n  --silent \\\n  --prefix /usr/local \\\n  --port 9180 \\\n  --flight-port 9181 \\\n  --database temnion \\\n  --user admin \\\n  --password "Secr3tP@ssword!"`}
                    lang="bash"
                    title="Silent Bash Setup"
                  />
                </>
              )}

              {installPlatform === "cargo" && (
                <>
                  <h3>Building from Source via Cargo</h3>
                  <p className="docs-paragraph">
                    Compile all release binaries from the workspace using the Rust toolchain:
                  </p>
                  <CodeSnippet
                    code={`git clone https://github.com/William-Avery/temnion.git\ncd temnion\n\n# Build all workspace release binaries\ncargo build --release --locked\n\n# Binaries produced:\n# target/release/tem          (CLI)\n# target/release/temniond     (Daemon Server)\n# target/release/tzeentch     (Standalone Consumer Client)`}
                    lang="bash"
                    title="Cargo Workspace Build"
                  />
                </>
              )}

              <div className="docs-alert alert-note">
                <div className="alert-title">Strict Memory Safety Invariant</div>
                <div>
                  Every crate in the Temnion workspace is strictly compiled with <code>#![forbid(unsafe_code)]</code>. There are zero unsafe memory operations anywhere in the engine.
                </div>
              </div>
            </article>
          )}

          {/* Chapter 3: Daemon */}
          {chapter === "daemon" && (
            <article className="docs-chapter">
              <h2>🖥️ Database Daemon (`temniond`)</h2>
              <p className="docs-lead">
                <code>temniond</code> is the always-on server application that hosts network connections, handles multi-threaded queries, and runs background maintenance tasks.
              </p>

              <h3>Commands Overview</h3>
              <ul style={{ paddingLeft: "20px", marginBottom: "16px", color: "#cbd5e1", lineHeight: "1.8" }}>
                <li><code>temniond init</code>: Creates database storage directory and generates reference <code>temnion.toml</code>.</li>
                <li><code>temniond run</code>: Starts the server runtime in the foreground with graceful termination on <code>Ctrl+C</code>.</li>
                <li><code>temniond status</code>: Probes a live running daemon over the network without interrupting active queries.</li>
                <li><code>temniond version</code>: Displays compiler toolchain, commit hash, and safety flags.</li>
              </ul>

              <CodeSnippet
                code={`# Probe daemon health and storage statistics\ntemniond status --config ./temnion.toml`}
                lang="bash"
                title="Status Health Probe"
              />

              <h3>Configuration Reference (`temnion.toml`)</h3>
              <CodeSnippet
                code={`database_name = "temnion"\nadmin_user = "admin"\nauth_token = "s3cr3t_auth_t0ken"\n\n[storage]\ndata_dir = "./data/temnion_db"\nmax_wal_bytes = 67108864 # 64 MiB WAL ceiling\nflush_interval_ms = 1000  # Fsync period\n\n[network]\ntnp_bind = "127.0.0.1:9180"    # TNP Protocol Port\nflight_bind = "127.0.0.1:9181" # Arrow Flight SQL Port\n\n[maintenance]\ninterval_secs = 60             # Checkpoint and WAL retirement sweep`}
                lang="toml"
                title="temnion.toml"
              />

              <h3>System Service Registration</h3>
              <p className="docs-paragraph">
                On Windows, run <code>pwsh services/windows/install-service.ps1</code> to register the Windows Service with automatic restart recovery. On Linux, copy <code>services/systemd/temniond.service</code> to <code>/etc/systemd/system/</code>.
              </p>
            </article>
          )}

          {/* Chapter 4: Connections */}
          {chapter === "connections" && (
            <article className="docs-chapter">
              <h2>🔌 Connections & Networking</h2>
              <p className="docs-lead">
                Connect from any application using standard connection strings, configuration profiles, or environment variables.
              </p>

              <h3>Connection URIs</h3>
              <p className="docs-paragraph">
                Temnion connection strings follow standard database URI conventions:
              </p>
              <CodeSnippet
                code={`temnion://[user[:password]@]host[:port]/database\n\n# Examples:\ntemnion://127.0.0.1:9180/temnion\ntemnion://admin:secret@localhost:9180/production\ntemnion://reader:token@db.internal:9180/telemetry_db`}
                lang="text"
                title="URI Specification"
              />

              <h3>Configuration Profiles (`~/.temnion/connections.toml`)</h3>
              <p className="docs-paragraph">
                Define reusable server profiles on your workstation:
              </p>
              <CodeSnippet
                code={`[default]\nhost = "127.0.0.1"\nport = 9180\nflight_port = 9181\ndatabase = "temnion"\nusername = "admin"\nauth_token = "s3cr3t_auth_t0ken"\n\n[production]\nhost = "prod-db.internal"\nport = 9180\nflight_port = 9181\ndatabase = "analytics_prod"\nusername = "admin"\nauth_token = "prod_tok_99182"`}
                lang="toml"
                title="~/.temnion/connections.toml"
              />

              <div className="docs-action-bar">
                <button
                  className="btn btn-primary"
                  onClick={() => onNavigate("connections")}
                  type="button"
                >
                  ◎ Open Studio Connections Manager
                </button>
              </div>
            </article>
          )}

          {/* Chapter 5: CLI */}
          {chapter === "cli" && (
            <article className="docs-chapter">
              <h2>💻 Command-Line Interface (`tem`)</h2>
              <p className="docs-lead">
                The <code>tem</code> utility enables scriptable command-line interaction for data exploration, batch ingestion, and branch management.
              </p>

              <h3>Basic Inspection</h3>
              <CodeSnippet
                code={`# Test connectivity and measure round-trip latency\ntem ping --uri temnion://127.0.0.1:9180/temnion\n\n# Display store summary metrics and WAL status\ntem status`}
                lang="bash"
                title="Ping & Status"
              />

              <h3>Writing Records & Events</h3>
              <CodeSnippet
                code={`# Ingest entity observation record\ntem ingest \\\n  --schema "sensor_telemetry" \\\n  --entity "sensor_42" \\\n  --fields '{"temperature": 24.5, "humidity": 58.2, "status": "nominal"}'`}
                lang="bash"
                title="Ingest Record"
              />

              <h3>Querying</h3>
              <CodeSnippet
                code={`# Standard SQL query\ntem query --sql "SELECT * FROM sensor_telemetry WHERE entity_id = 'sensor_42'"\n\n# Flashback point-in-time query\ntem query --sql "SELECT * FROM sensor_telemetry AS OF SYSTEM_TIME '2026-09-08T00:00:00Z'"`}
                lang="bash"
                title="SQL Query"
              />
            </article>
          )}

          {/* Chapter 6: Studio */}
          {chapter === "studio" && (
            <article className="docs-chapter">
              <h2>🎨 Studio Workbench Tour</h2>
              <p className="docs-lead">
                Temnion Studio is the official database administration and query explorer interface (equivalent to MySQL Workbench or pgAdmin).
              </p>

              <h3>Main Workspaces</h3>
              <ul style={{ paddingLeft: "20px", marginBottom: "16px", color: "#cbd5e1", lineHeight: "1.8" }}>
                <li><strong>⚡ Query Studio</strong>: Execute SQL, TemQL, or Compact queries with planning explain passes and bounded tabular results.</li>
                <li><strong>⊞ Schema & Entity Catalog</strong>: Inspect declared schemas, column layouts, Bloom filter point indices, and ZoneMap boundaries.</li>
                <li><strong>◴ Temporal Plane</strong>: Scrub through the multi-clock timeline across physical monotonic time and logical sequence coordinates.</li>
                <li><strong>⑂ Branches & DAG</strong>: Inspect timeline branches, cryptographic causality vectors, and fork speculative child branches.</li>
                <li><strong>◎ Connections Manager</strong>: Manage server profiles, perform live ping latency probes, and switch target databases.</li>
                <li><strong>▥ Storage & Capabilities</strong>: View authoritative store metrics, manifest-guarded WAL retirement status, and engine capability flags.</li>
              </ul>

              <div className="docs-action-bar">
                <button
                  className="btn btn-primary"
                  onClick={() => onNavigate("query")}
                  type="button"
                >
                  ⚡ Launch Query Studio
                </button>
                <button
                  className="btn btn-secondary"
                  onClick={() => onNavigate("schemas")}
                  type="button"
                >
                  ⊞ Browse Schemas
                </button>
              </div>
            </article>
          )}

          {/* Chapter 7: SQL & TemQL */}
          {chapter === "sql" && (
            <article className="docs-chapter">
              <h2>📜 SQL & TemQL Query Language Reference</h2>
              <p className="docs-lead">
                Temnion provides first-class support for temporal SQL predicates as well as native TemQL pipeline expressions.
              </p>

              <h3>Point-in-Time Flashback (`AS OF SYSTEM_TIME`)</h3>
              <p className="docs-paragraph">
                Reconstructs the authoritative state of entities at an exact physical nanosecond timestamp:
              </p>
              <CodeSnippet
                code={`SELECT entity_id, temperature, status\nFROM sensor_telemetry\nAS OF SYSTEM_TIME '2026-09-08T03:30:00.000Z'\nWHERE entity_id = 'sensor_42';`}
                lang="sql"
                title="AS OF SYSTEM_TIME"
              />

              <h3>Physical Interval Scanning</h3>
              <p className="docs-paragraph">
                Scans all event records committed between two physical nanosecond timestamps:
              </p>
              <CodeSnippet
                code={`SELECT entity_id, temperature, humidity\nFROM sensor_telemetry\nBETWEEN PHYSICAL 1757300000000000000 AND 1757303600000000000\nORDER BY physical_time DESC;`}
                lang="sql"
                title="BETWEEN PHYSICAL"
              />

              <h3>Causal Horizon Joins</h3>
              <p className="docs-paragraph">
                Filters records to only include those causally descended from a specific cryptographic parent hash:
              </p>
              <CodeSnippet
                code={`SELECT *\nFROM action_decision\nWITHIN CAUSAL HORIZON 'b3a8d9f1...'\nLIMIT 50;`}
                lang="sql"
                title="WITHIN CAUSAL HORIZON"
              />

              <h3>TemQL Pipeline Syntax</h3>
              <CodeSnippet
                code={`FROM sensor_telemetry\nWHERE temperature >= 25.0 AND status == "nominal"\nAS_OF PHYSICAL 1757301000000000000\nSELECT entity_id, temperature, humidity\nORDER BY temperature DESC\nLIMIT 25;`}
                lang="text"
                title="TemQL Expression"
              />

              <div className="docs-action-bar">
                <button
                  className="btn btn-primary"
                  onClick={() =>
                    onNavigate(
                      "query",
                      "SELECT entity_id, temperature, status\nFROM sensor_telemetry\nAS OF SYSTEM_TIME '2026-09-08T03:30:00.000Z'\nWHERE entity_id = 'sensor_42'"
                    )
                  }
                  type="button"
                >
                  ⚡ Try SQL in Query Studio
                </button>
              </div>
            </article>
          )}

          {/* Chapter 8: Tzeentch */}
          {chapter === "tzeentch" && (
            <article className="docs-chapter">
              <h2>🤖 Standalone Cognitive Client (`tzeentch`)</h2>
              <p className="docs-lead">
                <code>tzeentch</code> is an independent cognitive consumer client application entirely separate from the database engine and Temnion Studio IDE.
              </p>

              <div className="docs-alert alert-note">
                <div className="alert-title">Separation of Concerns</div>
                <div>
                  Just like Redis CLI tools or MySQL workbench clients are independent of the core server, <code>tzeentch</code> is a dedicated consumer client that talks to Temnion strictly over network protocol (TNP).
                </div>
              </div>

              <h3>Commands</h3>
              <CodeSnippet
                code={`# Test connection to Temnion server\ntzeentch ping --host 127.0.0.1 --port 9180\n\n# Query cognitive consumer state & server statistics\ntzeentch status\n\n# Track real-time stream ingestion cadence & latency\ntzeentch cadence --interval 5\n\n# Trace causal decision history for a sequence number\ntzeentch trace 42`}
                lang="bash"
                title="Tzeentch CLI Commands"
              />
            </article>
          )}

          {/* Chapter 9: Durability */}
          {chapter === "durability" && (
            <article className="docs-chapter">
              <h2>🛡️ Durability, Storage & Production Maintenance</h2>
              <p className="docs-lead">
                Temnion achieves high write throughput and crash resilience through a tiered storage architecture: Write-Ahead Log (WAL) and compressed columnar TSF segments.
              </p>

              <h3>WAL Retirement & Compaction</h3>
              <p className="docs-paragraph">
                When active WAL frames reach the configured size ceiling (e.g. 64 MiB), the engine seals them into immutable <code>.tsf</code> segment files and registers them in <code>SegmentManifest</code> (<code>segments/manifest.bin</code>).
              </p>
              <p className="docs-paragraph">
                Executing <code>Store::retire_wal</code> truncates the retired WAL prefix while strictly honoring any active <code>ReferenceHold</code> locks protecting active reader cursors or backups.
              </p>

              <h3>Physical Backups</h3>
              <CodeSnippet
                code={`# Create physical consistent backup snapshot\ntem backup create --dest /var/backups/temnion/snap_01\n\n# Verify manifest and segment CRC32 checksums\ntem backup verify --src /var/backups/temnion/snap_01\n\n# Restore into target directory\ntem backup restore --src /var/backups/temnion/snap_01 --dest /var/lib/temnion/data`}
                lang="bash"
                title="Backup and Restore"
              />

              <div className="docs-action-bar">
                <button
                  className="btn btn-primary"
                  onClick={() => onNavigate("metrics")}
                  type="button"
                >
                  ▥ View Storage & WAL Status
                </button>
              </div>
            </article>
          )}
        </main>
      </div>
    </section>
  );
}
