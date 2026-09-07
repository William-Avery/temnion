// SPDX-License-Identifier: AGPL-3.0-only

import { useMemo, useState } from "react";
import {
  type ColumnDef,
  flexRender,
  getCoreRowModel,
  useReactTable,
} from "@tanstack/react-table";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import {
  type AppendRequest,
  type BranchInfo,
  type EngineStatus,
  type EventRow,
  appendEvent,
  browserStatus,
  connectDatabase,
  createDatabase,
  disconnectDatabase,
  executeQuery,
  explainQuery,
  getEngineStatus,
  isNativeRuntime,
  listBranches,
  listHistory,
  traceCausality,
} from "./api";

type View = "query" | "history" | "causality" | "ingest" | "connections" | "metrics";
type QueryFormat = "temql" | "compact" | "sql";

const samples: Record<QueryFormat, string> = {
  temql: "FROM temnion\nSELECT entity, schema, valid_time, known_time, sequence\nLIMIT 100",
  compact: "tn:>entity,schema,valid_time,known_time,sequence!100",
  sql: "SELECT entity, schema, valid_time, known_time, sequence\nFROM temnion\nLIMIT 100",
};

const navItems: Array<{ id: View; label: string; icon: string; group: string }> = [
  { id: "query", label: "Query Studio", icon: "⌁", group: "Explore" },
  { id: "history", label: "Temporal Plane", icon: "◴", group: "Explore" },
  { id: "causality", label: "Branches & Causality", icon: "⑂", group: "Explore" },
  { id: "ingest", label: "Ingestion", icon: "⇧", group: "Data" },
  { id: "connections", label: "Connections", icon: "◎", group: "Operate" },
  { id: "metrics", label: "Storage & Capabilities", icon: "▥", group: "Operate" },
];

function formatBytes(bytes: number): string {
  if (bytes < 1_024) return `${bytes} B`;
  if (bytes < 1_048_576) return `${(bytes / 1_024).toFixed(1)} KiB`;
  return `${(bytes / 1_048_576).toFixed(1)} MiB`;
}

function errorText(error: unknown): string {
  if (error instanceof Error) return error.message;
  return String(error);
}

function Logo() {
  return (
    <div className="header-brand">
      <div className="brand-logo" aria-hidden="true">
        <svg width="26" height="26" viewBox="0 0 24 24" fill="none">
          <path d="M12 2 2 7l10 5 10-5-10-5Z" stroke="#54d8ff" strokeWidth="1.8" />
          <path d="m2 12 10 5 10-5M2 17l10 5 10-5" stroke="#7c8cff" strokeWidth="1.8" />
        </svg>
      </div>
      <div className="brand-title">
        <span className="title-primary">TEMNION</span>
        <span className="title-sub">STUDIO</span>
        <span className="version-badge">v0.1</span>
      </div>
    </div>
  );
}

function Sidebar({ view, onChange }: { view: View; onChange: (view: View) => void }) {
  const groups = ["Explore", "Data", "Operate"];
  return (
    <nav className="studio-sidebar" aria-label="Studio sections">
      {groups.map((group) => (
        <div className="nav-group" key={group}>
          <div className="nav-label">{group}</div>
          {navItems
            .filter((item) => item.group === group)
            .map((item) => (
              <button
                className={`nav-item ${view === item.id ? "active" : ""}`}
                key={item.id}
                onClick={() => onChange(item.id)}
                type="button"
              >
                <span className="nav-icon" aria-hidden="true">{item.icon}</span>
                <span>{item.label}</span>
              </button>
            ))}
        </div>
      ))}
      <div className="sidebar-footer">
        <div className="engine-badge">
          <div className="engine-indicator" />
          <div>
            <div className="engine-label">Bounded native commands</div>
            <div className="engine-sub">Rust · React · TanStack</div>
          </div>
        </div>
      </div>
    </nav>
  );
}

function Notice({ kind = "info", children }: { kind?: "info" | "error" | "success"; children: React.ReactNode }) {
  return <div className={`notice notice-${kind}`}>{children}</div>;
}

function EmptyState({ children }: { children: React.ReactNode }) {
  return <div className="empty-state">{children}</div>;
}

function EventTable({ rows, showPayload = false }: { rows: EventRow[]; showPayload?: boolean }) {
  const columns = useMemo<ColumnDef<EventRow>[]>(() => {
    const base: ColumnDef<EventRow>[] = [
      { accessorKey: "sequence", header: "Sequence" },
      { accessorKey: "entity", header: "Entity" },
      { accessorKey: "schema", header: "Schema" },
      {
        id: "validTime",
        header: "Valid time",
        cell: ({ row }) => `${row.original.validClock}:${row.original.validTime}`,
      },
      {
        id: "knownTime",
        header: "Known time",
        cell: ({ row }) => `${row.original.knownClock}:${row.original.knownTime}`,
      },
      {
        id: "fields",
        header: "Fields",
        cell: ({ row }) =>
          row.original.fields.length
            ? row.original.fields.map((field) => `${field.name}=${field.value}`).join(", ")
            : "—",
      },
    ];
    if (showPayload) {
      base.push({
        id: "payload",
        header: "Payload preview",
        cell: ({ row }) => `${row.original.payloadHex || "—"} (${row.original.payloadBytes} B)`,
      });
      base.push({
        id: "causes",
        header: "Causes",
        cell: ({ row }) => row.original.causes.join(", ") || "—",
      });
    }
    return base;
  }, [showPayload]);

  const table = useReactTable({ data: rows, columns, getCoreRowModel: getCoreRowModel() });

  if (!rows.length) {
    return <EmptyState>No rows loaded. Results remain empty until a bounded native query completes.</EmptyState>;
  }

  return (
    <div className="table-scroll">
      <table className="studio-table">
        <thead>
          {table.getHeaderGroups().map((group) => (
            <tr key={group.id}>
              {group.headers.map((header) => (
                <th key={header.id}>
                  {header.isPlaceholder ? null : flexRender(header.column.columnDef.header, header.getContext())}
                </th>
              ))}
            </tr>
          ))}
        </thead>
        <tbody>
          {table.getRowModel().rows.map((row) => (
            <tr key={row.id}>
              {row.getVisibleCells().map((cell) => (
                <td key={cell.id}>{flexRender(cell.column.columnDef.cell, cell.getContext())}</td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function PanelHeader({ title, subtitle, action }: { title: string; subtitle: string; action?: React.ReactNode }) {
  return (
    <div className="panel-header">
      <div className="panel-title-group">
        <h2>{title}</h2>
        <p className="panel-subtitle">{subtitle}</p>
      </div>
      {action}
    </div>
  );
}

function QueryPanel({ connected }: { connected: boolean }) {
  const [format, setFormat] = useState<QueryFormat>("temql");
  const [query, setQuery] = useState(samples.temql);
  const [maxRows, setMaxRows] = useState(100);
  const [bookmarks, setBookmarks] = useState<string[]>(() => {
    try {
      return JSON.parse(localStorage.getItem("temnion-query-bookmarks") ?? "[]") as string[];
    } catch {
      return [];
    }
  });
  const run = useMutation({ mutationFn: () => executeQuery(query, maxRows) });
  const explain = useMutation({ mutationFn: () => explainQuery(query) });

  const chooseFormat = (next: QueryFormat) => {
    setFormat(next);
    setQuery(samples[next]);
    run.reset();
    explain.reset();
  };

  const saveBookmark = () => {
    const next = [query, ...bookmarks.filter((value) => value !== query)].slice(0, 8);
    setBookmarks(next);
    localStorage.setItem("temnion-query-bookmarks", JSON.stringify(next));
  };

  return (
    <section className="view-panel active">
      <PanelHeader
        title="Query Studio"
        subtitle="TemQL, compact Tem, and SQL lower through the same canonical typed query IR."
        action={
          <div className="frontend-tabs" aria-label="Query language">
            {(["temql", "compact", "sql"] as QueryFormat[]).map((item) => (
              <button
                className={`frontend-tab ${format === item ? "active" : ""}`}
                key={item}
                onClick={() => chooseFormat(item)}
                type="button"
              >
                {item === "compact" ? "Compact tn:" : item.toUpperCase()}
              </button>
            ))}
          </div>
        }
      />
      {!connected && <Notice>Connect an existing database or create one before executing queries.</Notice>}
      <div className="editor-container glass-card">
        <div className="editor-toolbar">
          <div className="toolbar-left">
            <span className="lang-tag">{format}</span>
            <button className="tool-btn" type="button" onClick={() => setQuery(samples[format])}>Load sample</button>
            <button className="tool-btn" type="button" onClick={saveBookmark}>Bookmark</button>
          </div>
          <div className="toolbar-right bounded-label">Hard result ceiling: 1,000 rows</div>
        </div>
        <textarea
          aria-label="Query text"
          className="code-editor"
          spellCheck={false}
          value={query}
          onChange={(event) => setQuery(event.target.value)}
        />
        <div className="editor-footer">
          <label className="budget-label">
            Max rows
            <input
              className="num-input"
              max={1_000}
              min={1}
              type="number"
              value={maxRows}
              onChange={(event) => setMaxRows(Number(event.target.value))}
            />
          </label>
          <div className="exec-actions">
            <button className="btn btn-secondary" disabled={explain.isPending} onClick={() => explain.mutate()} type="button">
              {explain.isPending ? "Planning…" : "Explain"}
            </button>
            <button className="btn btn-primary" disabled={!connected || run.isPending} onClick={() => run.mutate()} type="button">
              {run.isPending ? "Executing…" : "Execute query"}
            </button>
          </div>
        </div>
      </div>
      {(run.error || explain.error) && <Notice kind="error">{errorText(run.error ?? explain.error)}</Notice>}
      {explain.data && <pre className="explain-output glass-card">{explain.data}</pre>}
      <div className="results-container glass-card">
        <div className="results-header">
          <div className="results-stats">
            <span><strong>{run.data?.rows.length ?? 0}</strong> rows</span>
            <span>•</span>
            <span><strong>{run.data?.eventsScanned ?? 0}</strong> scanned</span>
            <span>•</span>
            <span><strong>{formatBytes(run.data?.bytesRead ?? 0)}</strong> read</span>
            <span>•</span>
            <span>{((run.data?.elapsedMicros ?? 0) / 1_000).toFixed(2)} ms</span>
            {run.data?.truncated && <span className="badge badge-warning">Truncated</span>}
          </div>
        </div>
        <EventTable rows={run.data?.rows ?? []} />
      </div>
      {!!bookmarks.length && (
        <div className="glass-card bookmark-card">
          <div className="card-title">Local query bookmarks</div>
          {bookmarks.map((bookmark, index) => (
            <button className="bookmark" key={`${bookmark}-${index}`} onClick={() => setQuery(bookmark)} type="button">
              <code>{bookmark.replaceAll("\n", " ")}</code>
            </button>
          ))}
        </div>
      )}
    </section>
  );
}

function TemporalPlane({ rows }: { rows: EventRow[] }) {
  if (!rows.length) return <EmptyState>Load history to plot valid time against known time.</EmptyState>;
  const valid = rows.map((row) => row.validTime);
  const known = rows.map((row) => row.knownTime);
  const validMin = Math.min(...valid);
  const validSpan = Math.max(1, Math.max(...valid) - validMin);
  const knownMin = Math.min(...known);
  const knownSpan = Math.max(1, Math.max(...known) - knownMin);
  return (
    <svg className="temporal-plane" viewBox="0 0 820 300" role="img" aria-label="Valid time by known time event plot">
      <line x1="55" y1="255" x2="790" y2="255" className="axis-line" />
      <line x1="55" y1="20" x2="55" y2="255" className="axis-line" />
      <text x="690" y="285" className="axis-label">valid time →</text>
      <text x="12" y="18" className="axis-label">known ↑</text>
      {rows.map((row) => {
        const x = 65 + ((row.validTime - validMin) / validSpan) * 710;
        const y = 245 - ((row.knownTime - knownMin) / knownSpan) * 210;
        return (
          <g key={row.eventId || row.sequence}>
            <circle cx={x} cy={y} r="6" className="event-dot" />
            <title>{`seq ${row.sequence}; entity ${row.entity}; valid ${row.validTime}; known ${row.knownTime}`}</title>
          </g>
        );
      })}
    </svg>
  );
}

function HistoryPanel({ connected, eventCount }: { connected: boolean; eventCount: number }) {
  const [maxRows, setMaxRows] = useState(100);
  const history = useQuery({
    queryKey: ["history", maxRows, eventCount],
    queryFn: () => listHistory(maxRows),
    enabled: connected,
  });
  return (
    <section className="view-panel active">
      <PanelHeader
        title="Bi-temporal history"
        subtitle="A bounded native scan plotted on independent valid-time and known-time axes."
        action={
          <label className="budget-label">
            Rows
            <input className="num-input" min={1} max={1_000} type="number" value={maxRows} onChange={(event) => setMaxRows(Number(event.target.value))} />
          </label>
        }
      />
      {!connected && <Notice>Connect a database to load durable history.</Notice>}
      {history.error && <Notice kind="error">{errorText(history.error)}</Notice>}
      <div className="glass-card plane-card">
        <TemporalPlane rows={history.data?.rows ?? []} />
      </div>
      <div className="results-container glass-card">
        <div className="results-header">
          <div className="results-stats">
            <span><strong>{history.data?.rows.length ?? 0}</strong> rows</span>
            <span>•</span>
            <span><strong>{history.data?.eventsScanned ?? 0}</strong> scanned</span>
            <span>•</span>
            <span><strong>{formatBytes(history.data?.bytesRead ?? 0)}</strong> read</span>
            {history.data?.hasMore && <span className="badge badge-warning">More rows available</span>}
          </div>
        </div>
        <EventTable rows={history.data?.rows ?? []} showPayload />
      </div>
    </section>
  );
}

function BranchCard({ branch }: { branch: BranchInfo }) {
  return (
    <div className="branch-item">
      <div className="b-name">{branch.name} <span className="badge badge-neutral">{branch.lifecycle}</span></div>
      <div className="b-meta">
        ID {branch.id} · {branch.parentId === undefined ? "root" : `parent ${branch.parentId} at sequence ${branch.forkSequence}`}
      </div>
    </div>
  );
}

function CausalityPanel({ connected, eventCount }: { connected: boolean; eventCount: number }) {
  const [sequence, setSequence] = useState(Math.max(0, eventCount - 1));
  const [depth, setDepth] = useState(8);
  const branches = useQuery({ queryKey: ["branches", eventCount], queryFn: listBranches, enabled: connected });
  const trace = useMutation({ mutationFn: () => traceCausality(sequence, depth) });
  return (
    <section className="view-panel active">
      <PanelHeader title="Branches & causal trace" subtitle="Read persistent branch metadata and trace declared causal edges from bounded durable history." />
      {!connected && <Notice>Connect a database to inspect branch or causal metadata.</Notice>}
      {(branches.error || trace.error) && <Notice kind="error">{errorText(branches.error ?? trace.error)}</Notice>}
      <div className="causal-layout">
        <div className="glass-card branch-list-card">
          <div className="card-title">Timeline branches</div>
          {(branches.data ?? []).map((branch) => <BranchCard branch={branch} key={branch.id} />)}
          {!branches.data?.length && <EmptyState>No branch metadata loaded.</EmptyState>}
        </div>
        <div className="glass-card causal-graph-card">
          <div className="card-title">Trace an event</div>
          <div className="inline-form">
            <label>Sequence<input className="num-input" min={0} type="number" value={sequence} onChange={(event) => setSequence(Number(event.target.value))} /></label>
            <label>Depth<input className="num-input" min={1} max={32} type="number" value={depth} onChange={(event) => setDepth(Number(event.target.value))} /></label>
            <button className="btn btn-primary" disabled={!connected || trace.isPending} onClick={() => trace.mutate()} type="button">Trace</button>
          </div>
          {trace.data ? (
            <div className="trace-grid">
              <div><span className="eyebrow">Root</span><code>{trace.data.root}</code></div>
              <div><span className="eyebrow">Upstream causes</span>{trace.data.causes.map((node) => <code key={node.eventId}>{node.eventId} · depth {node.depth}</code>)}</div>
              <div><span className="eyebrow">Downstream effects</span>{trace.data.effects.map((node) => <code key={node.eventId}>{node.eventId} · depth {node.depth}</code>)}</div>
              <div><span className="eyebrow">Edges</span>{trace.data.edges.map((edge) => <code key={edge}>{edge}</code>)}</div>
            </div>
          ) : <EmptyState>Enter an existing source-local sequence to inspect its causal cone.</EmptyState>}
        </div>
      </div>
    </section>
  );
}

function IngestPanel({ connected }: { connected: boolean }) {
  const queryClient = useQueryClient();
  const [form, setForm] = useState<AppendRequest>({
    entity: "0:1:0",
    schema: 1,
    validTime: "1:0",
    knownTime: "1:0",
    payloadHex: "00",
    causes: [],
  });
  const [causes, setCauses] = useState("");
  const append = useMutation({
    mutationFn: () => appendEvent({ ...form, causes: causes.split(",").map((cause) => cause.trim()).filter(Boolean) }),
    onSuccess: (receipt) => {
      queryClient.setQueryData(["engine-status"], receipt.status);
      void queryClient.invalidateQueries({ queryKey: ["history"] });
      void queryClient.invalidateQueries({ queryKey: ["branches"] });
    },
  });
  const update = <K extends keyof AppendRequest>(key: K, value: AppendRequest[K]) => setForm((current) => ({ ...current, [key]: value }));
  return (
    <section className="view-panel active">
      <PanelHeader title="Deterministic ingestion" subtitle="Validate and append one bounded event; success is reported only after the durable store acknowledges OS synchronization." />
      {!connected && <Notice>Connect a database before appending an event.</Notice>}
      {append.error && <Notice kind="error">{errorText(append.error)}</Notice>}
      {append.data && <Notice kind="success">Committed {append.data.count} event as {append.data.firstEvent}.</Notice>}
      <div className="glass-card ingest-form-card">
        <div className="form-grid">
          <label className="form-group">Entity (shard:slot:generation)<input className="text-input" value={form.entity} onChange={(event) => update("entity", event.target.value)} /></label>
          <label className="form-group">Schema ID<input className="num-input" min={0} type="number" value={form.schema} onChange={(event) => update("schema", Number(event.target.value))} /></label>
          <label className="form-group">Valid time (clock:tick)<input className="text-input" value={form.validTime} onChange={(event) => update("validTime", event.target.value)} /></label>
          <label className="form-group">Known time (clock:tick)<input className="text-input" value={form.knownTime} onChange={(event) => update("knownTime", event.target.value)} /></label>
        </div>
        <label className="form-group">Cause IDs (source:epoch:sequence, comma-separated)<input className="text-input" value={causes} onChange={(event) => setCauses(event.target.value)} /></label>
        <label className="form-group">Hex payload<textarea className="code-editor compact-editor" spellCheck={false} value={form.payloadHex} onChange={(event) => update("payloadHex", event.target.value)} /></label>
        <div className="form-actions">
          <button className="btn btn-primary" disabled={!connected || append.isPending} onClick={() => append.mutate()} type="button">
            {append.isPending ? "Committing…" : "Admit & commit event"}
          </button>
        </div>
      </div>
    </section>
  );
}

function ConnectionsPanel({ status }: { status: EngineStatus }) {
  const queryClient = useQueryClient();
  const [path, setPath] = useState(status.path ?? "");
  const connect = useMutation({
    mutationFn: (mode: "open" | "create") => mode === "open" ? connectDatabase(path) : createDatabase(path),
    onSuccess: (next) => queryClient.setQueryData(["engine-status"], next),
  });
  const disconnect = useMutation({
    mutationFn: disconnectDatabase,
    onSuccess: (next) => {
      queryClient.setQueryData(["engine-status"], next);
      queryClient.removeQueries({ queryKey: ["history"] });
      queryClient.removeQueries({ queryKey: ["branches"] });
    },
  });
  return (
    <section className="view-panel active">
      <PanelHeader title="Local connection" subtitle="The native Rust host owns the path, file handle, recovery checks, and writer lock; the webview receives bounded view models only." />
      {!isNativeRuntime && <Notice>This browser preview is intentionally disconnected. Run the Tauri desktop command to access local databases.</Notice>}
      {(connect.error || disconnect.error) && <Notice kind="error">{errorText(connect.error ?? disconnect.error)}</Notice>}
      <div className="glass-card connection-card">
        <label className="form-group">Database directory<input className="text-input path-input" placeholder="C:\data\temnion-example" value={path} onChange={(event) => setPath(event.target.value)} /></label>
        <div className="connection-actions">
          <button className="btn btn-primary" disabled={connect.isPending || !isNativeRuntime} onClick={() => connect.mutate("open")} type="button">Open existing</button>
          <button className="btn btn-secondary" disabled={connect.isPending || !isNativeRuntime} onClick={() => connect.mutate("create")} type="button">Create new</button>
          <button className="btn btn-secondary" disabled={!status.connected || disconnect.isPending} onClick={() => disconnect.mutate()} type="button">Disconnect</button>
        </div>
      </div>
      <div className="connections-grid">
        <div className={`glass-card conn-card ${status.connected ? "active-conn" : ""}`}>
          <div className="conn-header"><div className="conn-type">Embedded local store</div><span className={`badge ${status.connected ? "badge-success" : "badge-neutral"}`}>{status.connected ? "Active" : "Disconnected"}</span></div>
          <div className="conn-path"><code>{status.path ?? "No path selected"}</code></div>
          <div className="conn-meta">Direct native Rust integration; no listener is opened.</div>
        </div>
        <div className="glass-card conn-card">
          <div className="conn-header"><div className="conn-type">TNP / Arrow Flight / MCP</div><span className="badge badge-neutral">External</span></div>
          <div className="conn-meta">The engine supports these interfaces, but this Studio increment deliberately enables no remote connection by default.</div>
        </div>
      </div>
    </section>
  );
}

function MetricsPanel({ status }: { status: EngineStatus }) {
  return (
    <section className="view-panel active">
      <PanelHeader title="Storage & capabilities" subtitle="Live values come from the active durable Store handle; advertised capabilities are limited to implemented Studio commands." />
      <div className="metrics-grid">
        <div className="glass-card metric-card"><div className="metric-val">{status.eventCount}</div><div className="metric-label">Durable events</div></div>
        <div className="glass-card metric-card"><div className="metric-val">{formatBytes(status.walBytes)}</div><div className="metric-label">WAL prefix</div></div>
        <div className="glass-card metric-card"><div className="metric-val">{status.summaryBlocks}</div><div className="metric-label">Summary blocks</div></div>
        <div className="glass-card metric-card"><div className="metric-val">{status.maxRows}</div><div className="metric-label">Maximum rows per view</div></div>
      </div>
      <div className="glass-card capabilities-card">
        <div className="card-title">Native Studio capability surface</div>
        <div className="capability-list">{status.capabilities.map((capability) => <span className="badge badge-neutral" key={capability}>{capability}</span>)}</div>
        <dl className="identity-grid">
          <div><dt>Database</dt><dd>{status.databaseId ?? "—"}</dd></div>
          <div><dt>Source</dt><dd>{status.source ?? "—"}</dd></div>
          <div><dt>Epoch</dt><dd>{status.epoch ?? "—"}</dd></div>
          <div><dt>Unsafe Rust</dt><dd>Forbidden</dd></div>
        </dl>
      </div>
      <Notice>Persistent schema registration, 3D Morton rendering, branch mutation, native installers, and ARM64 desktop qualification remain future M23 work and are not advertised as complete.</Notice>
    </section>
  );
}

export function App() {
  const [view, setView] = useState<View>("query");
  const statusQuery = useQuery({ queryKey: ["engine-status"], queryFn: getEngineStatus, initialData: browserStatus });
  const status = statusQuery.data;
  let content: React.ReactNode;
  switch (view) {
    case "query": content = <QueryPanel connected={status.connected} />; break;
    case "history": content = <HistoryPanel connected={status.connected} eventCount={status.eventCount} />; break;
    case "causality": content = <CausalityPanel connected={status.connected} eventCount={status.eventCount} />; break;
    case "ingest": content = <IngestPanel connected={status.connected} />; break;
    case "connections": content = <ConnectionsPanel status={status} />; break;
    case "metrics": content = <MetricsPanel status={status} />; break;
  }
  return (
    <div className="studio-layout">
      <header className="studio-header">
        <Logo />
        <div className="header-status">
          <div className={`status-chip ${status.connected ? "connected" : ""}`}>
            <span className="pulse-dot" />
            <span>{status.connected ? `Embedded · ${status.eventCount} events` : "No database connected"}</span>
          </div>
          <div className="status-chip branch-chip">main timeline</div>
          <div className="status-chip mode-chip">bounded exact views</div>
        </div>
        <div className="header-actions">
          <button className="btn btn-secondary btn-sm" onClick={() => setView("connections")} type="button">{status.connected ? "Change database" : "Connect"}</button>
          <button className="btn btn-primary btn-sm" onClick={() => setView("query")} type="button">New query</button>
        </div>
      </header>
      <div className="studio-body">
        <Sidebar view={view} onChange={setView} />
        <main className="studio-content">{content}</main>
      </div>
    </div>
  );
}
