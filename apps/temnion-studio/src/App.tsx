// SPDX-License-Identifier: AGPL-3.0-only

import { useEffect, useMemo, useState } from "react";
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
  type ConnectionProfile,
  type EngineStatus,
  type EventRow,
  appendEvent,
  browserStatus,
  connectDatabase,
  createDatabase,
  deleteConnection,
  disconnectDatabase,
  executeQuery,
  explainQuery,
  getEngineStatus,
  isNativeRuntime,
  listBranches,
  listConnections,
  listEntities,
  listHistory,
  listSchemas,
  saveConnection,
  setActiveConnection,
  testConnection,
  traceCausality,
} from "./api";
import { GuidePanel } from "./Guide";

type View = "guide" | "query" | "history" | "causality" | "schemas" | "ingest" | "connections" | "metrics";
type QueryFormat = "temql" | "compact" | "sql";

const samples: Record<QueryFormat, string> = {
  temql: "FROM temnion\nSELECT entity, schema, valid_time, known_time, sequence\nLIMIT 100",
  compact: "tn:>entity,schema,valid_time,known_time,sequence!100",
  sql: "SELECT entity, schema, valid_time, known_time, sequence\nFROM temnion\nLIMIT 100",
};

const navItems: Array<{ id: View; label: string; icon: string; group: string }> = [
  { id: "guide", label: "How-To Guide", icon: "📖", group: "Learn & Docs" },
  { id: "query", label: "Query Studio", icon: "⚡", group: "Workbench" },
  { id: "schemas", label: "Schema Catalog", icon: "⊞", group: "Workbench" },
  { id: "history", label: "Temporal Plane", icon: "◴", group: "Workbench" },
  { id: "causality", label: "Branches & DAG", icon: "⑂", group: "Workbench" },
  { id: "connections", label: "Connections", icon: "◎", group: "Operate" },
  { id: "ingest", label: "Ingestion Console", icon: "⇧", group: "Operate" },
  { id: "metrics", label: "Storage & Health", icon: "▥", group: "Operate" },
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

interface QueryPanelProps {
  connected: boolean;
  initialQuery?: string;
  onNavigateToGuide: () => void;
  onNavigateToConnections: () => void;
}

const queryPresets = [
  {
    label: "Basic Scan (SQL)",
    fmt: "sql" as QueryFormat,
    text: "SELECT entity, schema, valid_time, known_time, sequence\nFROM temnion\nLIMIT 25",
  },
  {
    label: "Time Travel Flashback (SQL)",
    fmt: "sql" as QueryFormat,
    text: "SELECT entity, schema, valid_time, known_time, sequence\nFROM temnion\nAS OF SYSTEM_TIME '2026-09-08T00:00:00Z'\nLIMIT 50",
  },
  {
    label: "Physical Range (SQL)",
    fmt: "sql" as QueryFormat,
    text: "SELECT entity, schema, valid_time, known_time, sequence\nFROM temnion\nBETWEEN PHYSICAL 1757300000000000000 AND 1757305000000000000\nLIMIT 50",
  },
  {
    label: "TemQL Pipeline",
    fmt: "temql" as QueryFormat,
    text: "FROM temnion\nWHERE sequence > 0\nSELECT entity, schema, valid_time, known_time, sequence\nORDER BY sequence DESC\nLIMIT 25",
  },
];

function QueryPanel({
  connected,
  initialQuery,
  onNavigateToGuide,
  onNavigateToConnections,
}: QueryPanelProps) {
  const [format, setFormat] = useState<QueryFormat>(() => {
    if (initialQuery?.trim().toUpperCase().startsWith("SELECT")) return "sql";
    return "temql";
  });
  const [query, setQuery] = useState(initialQuery ?? samples.temql);
  const [maxRows, setMaxRows] = useState(100);
  const [bookmarks, setBookmarks] = useState<string[]>(() => {
    try {
      return JSON.parse(localStorage.getItem("temnion-query-bookmarks") ?? "[]") as string[];
    } catch {
      return [];
    }
  });

  useEffect(() => {
    if (initialQuery) {
      setQuery(initialQuery);
      if (initialQuery.trim().toUpperCase().startsWith("SELECT")) {
        setFormat("sql");
      } else if (initialQuery.trim().toUpperCase().startsWith("FROM")) {
        setFormat("temql");
      }
    }
  }, [initialQuery]);

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

      {/* Quickstart Hero Card */}
      <div className="quickstart-hero">
        <div className="quickstart-text">
          <h3>Interactive Temporal Query Studio</h3>
          <p>
            Execute SQL or native TemQL queries across physical wall-clock time, Lamport sequences, and causal branch DAGs. All operations are bounded and verified memory-safe under <code>#![forbid(unsafe_code)]</code>.
          </p>
        </div>
        <div className="quickstart-actions">
          <button className="btn btn-secondary btn-sm" onClick={onNavigateToGuide} type="button">
            📖 How-To Guide
          </button>
          <button className="btn btn-secondary btn-sm" onClick={onNavigateToConnections} type="button">
            ◎ Connections
          </button>
          <button
            className="btn btn-primary btn-sm"
            onClick={() => {
              setFormat("sql");
              setQuery(queryPresets[1].text);
              run.mutate();
            }}
            type="button"
          >
            ⚡ Run Time Travel Demo
          </button>
        </div>
      </div>

      {!connected && <Notice>Connect an existing database or create one before executing queries.</Notice>}

      {/* Query Presets Chips */}
      <div className="query-presets-bar">
        <span className="presets-label">Sample Presets:</span>
        {queryPresets.map((p) => (
          <button
            className="preset-chip"
            key={p.label}
            onClick={() => {
              setFormat(p.fmt);
              setQuery(p.text);
              run.reset();
              explain.reset();
            }}
            type="button"
          >
            {p.label}
          </button>
        ))}
      </div>

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
            <span className="stat-pill safety">forbid(unsafe_code)</span>
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

function SchemaPanel() {
  const schemasQuery = useQuery({ queryKey: ["schemas"], queryFn: listSchemas });
  const entitiesQuery = useQuery({ queryKey: ["entities"], queryFn: listEntities });
  const [selectedSchemaId, setSelectedSchemaId] = useState<number>(1);

  const schemas = schemasQuery.data ?? [];
  const entities = entitiesQuery.data ?? [];
  const selectedSchema = schemas.find((s) => s.id === selectedSchemaId) ?? schemas[0];

  return (
    <section className="view-panel active">
      <PanelHeader
        title="Schema & Entity Catalog"
        subtitle="Authoritative schemas, field layouts, index coverage, and registered entity slots."
      />

      {/* Schema Cards Grid */}
      <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(240px, 1fr))", gap: "12px", marginBottom: "16px" }}>
        {schemas.map((schema) => (
          <div
            key={schema.id}
            className={`glass-card ${selectedSchemaId === schema.id ? "active-conn" : ""}`}
            style={{ padding: "14px", cursor: "pointer", border: selectedSchemaId === schema.id ? "1px solid #38bdf8" : undefined }}
            onClick={() => setSelectedSchemaId(schema.id)}
          >
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "8px" }}>
              <span className="badge badge-neutral" style={{ fontWeight: "bold" }}>Schema #{schema.id}</span>
              <span className="badge badge-success">{schema.eventCount} events</span>
            </div>
            <div style={{ fontSize: "15px", fontWeight: "bold", color: "#e2e8f0", marginBottom: "4px" }}>
              {schema.name}
            </div>
            <div style={{ fontSize: "12px", color: "#94a3b8", lineHeight: "1.4" }}>
              {schema.description}
            </div>
          </div>
        ))}
      </div>

      {/* Selected Schema Fields Inspector */}
      {selectedSchema && (
        <div className="glass-card" style={{ padding: "16px", marginBottom: "16px" }}>
          <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "12px" }}>
            <div>
              <div style={{ fontSize: "16px", fontWeight: "bold", color: "#e2e8f0" }}>
                Field Layout: <span style={{ color: "#38bdf8" }}>{selectedSchema.name}</span> (Schema #{selectedSchema.id})
              </div>
              <div style={{ fontSize: "12px", color: "#94a3b8" }}>
                Canonical Clock: Clock #{selectedSchema.clockId} · Total Fields: {selectedSchema.fields.length}
              </div>
            </div>
            <span className="badge badge-neutral">Arrow Columnar Compatible</span>
          </div>

          <table className="events-table">
            <thead>
              <tr>
                <th>Field Name</th>
                <th>Logical Data Type</th>
                <th>Indexing</th>
                <th>Storage Layout</th>
              </tr>
            </thead>
            <tbody>
              {selectedSchema.fields.map((f) => (
                <tr key={f.name}>
                  <td style={{ fontFamily: "monospace", color: "#e2e8f0", fontWeight: "bold" }}>{f.name}</td>
                  <td><span className="badge badge-neutral">{f.type}</span></td>
                  <td>
                    {f.indexed ? (
                      <span className="badge badge-success">Indexed (ZoneMap + Bloom)</span>
                    ) : (
                      <span className="badge badge-neutral">Direct Scan</span>
                    )}
                  </td>
                  <td style={{ fontSize: "12px", color: "#94a3b8" }}>Fixed-Width Bitpacked</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {/* Entity Slot Registry */}
      <div className="glass-card" style={{ padding: "16px" }}>
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "12px" }}>
          <div>
            <div style={{ fontSize: "15px", fontWeight: "bold", color: "#e2e8f0" }}>
              Registered Entity Slots
            </div>
            <div style={{ fontSize: "12px", color: "#94a3b8" }}>
              Authoritative shard-local entity mapping with temporal sequence counters.
            </div>
          </div>
          <span className="badge badge-neutral">{entities.length} active slots</span>
        </div>

        <table className="events-table">
          <thead>
            <tr>
              <th>Entity ID</th>
              <th>Shard ID</th>
              <th>Slot Index</th>
              <th>Generation</th>
              <th>Schema Assigned</th>
              <th>Event Count</th>
              <th>Last Valid Time</th>
            </tr>
          </thead>
          <tbody>
            {entities.map((e) => (
              <tr key={e.id}>
                <td style={{ fontFamily: "monospace", color: "#38bdf8", fontWeight: "bold" }}>{e.id}</td>
                <td>Shard {e.shard}</td>
                <td>Slot #{e.slot}</td>
                <td>Gen {e.generation}</td>
                <td><span className="badge badge-neutral">Schema #{e.schemaId}</span></td>
                <td>{e.totalEvents} events</td>
                <td style={{ fontFamily: "monospace" }}>{e.lastValidTime} ticks</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  );
}

function ConnectionsPanel({
  status,
  onEditConnection,
}: {
  status: EngineStatus;
  onEditConnection?: (conn: ConnectionProfile) => void;
}) {
  const queryClient = useQueryClient();
  const connectionsQuery = useQuery({ queryKey: ["connections"], queryFn: listConnections });
  const connections = connectionsQuery.data ?? [];

  const [activeConnId, setActiveConnId] = useState("conn-local-primary");
  const [showAddForm, setShowAddForm] = useState(false);
  const [testStatus, setTestStatus] = useState<{ success: boolean; message: string; latencyMs: number } | null>(null);

  const [form, setForm] = useState<{
    id?: string;
    name: string;
    host: string;
    tnpPort: number;
    flightPort: number;
    database: string;
    username: string;
    authToken: string;
    tls: boolean;
  }>({
    id: undefined,
    name: "New Connection",
    host: "127.0.0.1",
    tnpPort: 9180,
    flightPort: 9181,
    database: "temnion_default",
    username: "temnion_admin",
    authToken: "",
    tls: false,
  });

  const [embeddedPath, setEmbeddedPath] = useState(status.path ?? "");

  const saveMutation = useMutation({
    mutationFn: (profile: ConnectionProfile) => saveConnection(profile),
    onSuccess: (updated) => {
      queryClient.setQueryData(["connections"], updated);
      setShowAddForm(false);
      setTestStatus(null);
    },
  });

  const deleteMutation = useMutation({
    mutationFn: (id: string) => deleteConnection(id),
    onSuccess: (updated) => queryClient.setQueryData(["connections"], updated),
  });

  const testMutation = useMutation({
    mutationFn: (profile: Partial<ConnectionProfile>) => testConnection(profile),
    onSuccess: (res) => {
      setTestStatus({
        success: res.success,
        message: `${res.message} (Round-trip: ${res.latencyMs} ms)`,
        latencyMs: res.latencyMs,
      });
    },
    onError: (err) => {
      setTestStatus({
        success: false,
        message: errorText(err),
        latencyMs: 0,
      });
    },
  });

  const switchMutation = useMutation({
    mutationFn: (id: string) => setActiveConnection(id),
    onSuccess: (active) => {
      setActiveConnId(active.id);
      queryClient.invalidateQueries({ queryKey: ["engine-status"] });
    },
  });

  const connectEmbedded = useMutation({
    mutationFn: (mode: "open" | "create") => mode === "open" ? connectDatabase(embeddedPath) : createDatabase(embeddedPath),
    onSuccess: (next) => queryClient.setQueryData(["engine-status"], next),
  });

  const disconnectEmbedded = useMutation({
    mutationFn: disconnectDatabase,
    onSuccess: (next) => {
      queryClient.setQueryData(["engine-status"], next);
      queryClient.removeQueries({ queryKey: ["history"] });
      queryClient.removeQueries({ queryKey: ["branches"] });
    },
  });

  const handleEditInline = (conn: ConnectionProfile) => {
    setForm({
      id: conn.id,
      name: conn.name,
      host: conn.host,
      tnpPort: conn.tnpPort,
      flightPort: conn.flightPort,
      database: conn.database,
      username: conn.username,
      authToken: conn.authToken ?? "",
      tls: conn.tls,
    });
    setShowAddForm(true);
  };

  const handleSave = () => {
    const profile: ConnectionProfile = {
      id: form.id || `conn-${Date.now()}`,
      name: form.name,
      host: form.host,
      tnpPort: form.tnpPort,
      flightPort: form.flightPort,
      database: form.database,
      username: form.username,
      authToken: form.authToken ? "••••••••" : undefined,
      tls: form.tls,
      lastConnected: "Just now",
      status: "connected",
      latencyMs: testStatus?.latencyMs ?? 0.38,
      serverVersion: "TNP v1 (temniond 0.1.0)",
    };
    saveMutation.mutate(profile);
  };

  return (
    <section className="view-panel active">
      <PanelHeader
        title="Connections Manager"
        subtitle="Manage database connections, configure network ports, superuser credentials, and test server latency."
      />

      {/* Top Action Bar */}
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "16px" }}>
        <div style={{ fontSize: "14px", color: "#94a3b8" }}>
          Configured server endpoints recognized by Temnion Studio Workbench, CLI, and external consumers.
        </div>
        <button
          className="btn btn-primary btn-sm"
          type="button"
          onClick={() => {
            if (onEditConnection) {
              onEditConnection({
                id: "",
                name: "New Connection",
                host: "127.0.0.1",
                tnpPort: 9180,
                flightPort: 9181,
                database: "temnion_default",
                username: "temnion_admin",
                authToken: "",
                tls: false,
                status: "disconnected",
                latencyMs: 0.38,
                serverVersion: "TNP v1 (temniond 0.1.0)",
              });
            } else {
              setShowAddForm(!showAddForm);
            }
          }}
        >
          {showAddForm ? "Cancel" : "+ Add New Connection"}
        </button>
      </div>

      {/* Test Status Alert */}
      {testStatus && (
        <Notice kind={testStatus.success ? "success" : "error"}>
          {testStatus.message}
        </Notice>
      )}

      {/* Add / Edit Connection Form */}
      {showAddForm && (
        <div className="glass-card" style={{ padding: "16px", marginBottom: "20px", border: "1px solid #38bdf8" }}>
          <div style={{ fontSize: "15px", fontWeight: "bold", color: "#e2e8f0", marginBottom: "12px" }}>
            {form.id ? `Edit Connection: ${form.name}` : "New Temnion Database Connection"}
          </div>
          <div className="form-grid" style={{ marginBottom: "12px" }}>
            <label className="form-group">
              Connection Name
              <input
                className="text-input"
                value={form.name}
                onChange={(e) => setForm({ ...form, name: e.target.value })}
                placeholder="e.g. Production Primary"
              />
            </label>
            <label className="form-group">
              Host / Address
              <input
                className="text-input"
                value={form.host}
                onChange={(e) => setForm({ ...form, host: e.target.value })}
                placeholder="127.0.0.1"
              />
            </label>
            <label className="form-group">
              TNP Network Port
              <input
                className="num-input"
                type="number"
                value={form.tnpPort}
                onChange={(e) => setForm({ ...form, tnpPort: Number(e.target.value) })}
                placeholder="9180"
              />
            </label>
            <label className="form-group">
              Arrow Flight Port
              <input
                className="num-input"
                type="number"
                value={form.flightPort}
                onChange={(e) => setForm({ ...form, flightPort: Number(e.target.value) })}
                placeholder="9181"
              />
            </label>
            <label className="form-group">
              Database Name
              <input
                className="text-input"
                value={form.database}
                onChange={(e) => setForm({ ...form, database: e.target.value })}
                placeholder="temnion_default"
              />
            </label>
            <label className="form-group">
              Username
              <input
                className="text-input"
                value={form.username}
                onChange={(e) => setForm({ ...form, username: e.target.value })}
                placeholder="temnion_admin"
              />
            </label>
            <label className="form-group">
              Password / Auth Token
              <input
                className="text-input"
                type="password"
                value={form.authToken}
                onChange={(e) => setForm({ ...form, authToken: e.target.value })}
                placeholder="••••••••••••"
              />
            </label>
            <label className="form-group" style={{ display: "flex", alignItems: "center", gap: "8px", marginTop: "24px" }}>
              <input
                type="checkbox"
                checked={form.tls}
                onChange={(e) => setForm({ ...form, tls: e.target.checked })}
              />
              <span style={{ fontSize: "12px", color: "#e2e8f0" }}>Enable TLS / SSL</span>
            </label>
          </div>

          <div style={{ display: "flex", gap: "10px", justifyContent: "flex-end" }}>
            <button
              className="btn btn-secondary btn-sm"
              type="button"
              disabled={testMutation.isPending}
              onClick={() => testMutation.mutate(form)}
            >
              {testMutation.isPending ? "Testing..." : "Test Connection"}
            </button>
            <button
              className="btn btn-primary btn-sm"
              type="button"
              onClick={handleSave}
            >
              Save Connection
            </button>
          </div>
        </div>
      )}

      {/* Saved Connections Grid */}
      <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(320px, 1fr))", gap: "14px", marginBottom: "24px" }}>
        {connections.map((conn) => {
          const isActive = conn.id === activeConnId;
          return (
            <div
              key={conn.id}
              className={`glass-card conn-card ${isActive ? "active-conn" : ""}`}
              style={{ padding: "16px" }}
            >
              <div className="conn-header" style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "8px" }}>
                <div style={{ fontWeight: "bold", fontSize: "15px", color: "#e2e8f0" }}>{conn.name}</div>
                <span className={`badge ${isActive ? "badge-success" : "badge-neutral"}`}>
                  {isActive ? "Active (Connected)" : conn.status}
                </span>
              </div>

              <div style={{ fontSize: "12px", fontFamily: "monospace", color: "#38bdf8", marginBottom: "10px" }}>
                temnion://{conn.username}@{conn.host}:{conn.tnpPort}/{conn.database}
              </div>

              <div style={{ display: "grid", gridTemplateColumns: "repeat(2, 1fr)", gap: "6px", fontSize: "11px", color: "#94a3b8", marginBottom: "14px" }}>
                <div>TNP Port: <span style={{ color: "#e2e8f0" }}>{conn.tnpPort}</span></div>
                <div>Flight Port: <span style={{ color: "#e2e8f0" }}>{conn.flightPort}</span></div>
                <div>Protocol: <span style={{ color: "#e2e8f0" }}>{conn.serverVersion ?? "TNP v1"}</span></div>
                <div>Latency: <span style={{ color: "#34d399" }}>{conn.latencyMs ? `${conn.latencyMs} ms` : "—"}</span></div>
              </div>

              <div style={{ display: "flex", gap: "8px", justifyContent: "flex-end", flexWrap: "wrap" }}>
                <button
                  className="btn btn-secondary btn-sm"
                  type="button"
                  onClick={() => testMutation.mutate(conn)}
                >
                  Test
                </button>
                <button
                  className="btn btn-secondary btn-sm"
                  type="button"
                  onClick={() => onEditConnection ? onEditConnection(conn) : handleEditInline(conn)}
                >
                  ⚙ Edit Properties
                </button>
                {conn.id !== "conn-local-primary" && (
                  <button
                    className="btn btn-secondary btn-sm"
                    type="button"
                    onClick={() => deleteMutation.mutate(conn.id)}
                  >
                    Delete
                  </button>
                )}
                <button
                  className={`btn ${isActive ? "btn-secondary" : "btn-primary"} btn-sm`}
                  type="button"
                  disabled={isActive || switchMutation.isPending}
                  onClick={() => switchMutation.mutate(conn.id)}
                >
                  {isActive ? "Active" : "Connect"}
                </button>
              </div>
            </div>
          );
        })}
      </div>

      {/* Embedded Directory Store Fallback */}
      <div className="glass-card" style={{ padding: "16px" }}>
        <div style={{ fontSize: "14px", fontWeight: "bold", color: "#e2e8f0", marginBottom: "6px" }}>
          Local Embedded Store (Direct Filesystem)
        </div>
        <div style={{ fontSize: "12px", color: "#94a3b8", marginBottom: "12px" }}>
          Open a local database directory directly in embedded mode without running a server daemon.
        </div>
        <label className="form-group">
          Database Directory Path
          <input
            className="text-input path-input"
            placeholder="C:\ProgramData\Temnion\data or ./data/temnion_db"
            value={embeddedPath}
            onChange={(e) => setEmbeddedPath(e.target.value)}
          />
        </label>
        <div style={{ display: "flex", gap: "8px", marginTop: "10px" }}>
          <button
            className="btn btn-primary btn-sm"
            disabled={connectEmbedded.isPending || !isNativeRuntime}
            onClick={() => connectEmbedded.mutate("open")}
            type="button"
          >
            Open Existing Store
          </button>
          <button
            className="btn btn-secondary btn-sm"
            disabled={connectEmbedded.isPending || !isNativeRuntime}
            onClick={() => connectEmbedded.mutate("create")}
            type="button"
          >
            Create New Store
          </button>
          <button
            className="btn btn-secondary btn-sm"
            disabled={!status.connected || disconnectEmbedded.isPending}
            onClick={() => disconnectEmbedded.mutate()}
            type="button"
          >
            Disconnect
          </button>
        </div>
      </div>
    </section>
  );
}

function MetricsPanel({ status }: { status: EngineStatus }) {
  return (
    <section className="view-panel active">
      <PanelHeader
        title="Storage & Capabilities"
        subtitle="Authoritative store metrics, WAL retirement status, manifest metadata, and advertised capabilities."
      />
      <div className="metrics-grid">
        <div className="glass-card metric-card">
          <div className="metric-val">{status.eventCount}</div>
          <div className="metric-label">Durable events</div>
        </div>
        <div className="glass-card metric-card">
          <div className="metric-val">{formatBytes(status.walBytes)}</div>
          <div className="metric-label">Active WAL prefix</div>
        </div>
        <div className="glass-card metric-card">
          <div className="metric-val">{status.summaryBlocks}</div>
          <div className="metric-label">Retired Segments (TSF)</div>
        </div>
        <div className="glass-card metric-card">
          <div className="metric-val">{status.maxRows}</div>
          <div className="metric-label">Maximum rows per view</div>
        </div>
      </div>

      <div className="glass-card capabilities-card">
        <div className="card-title">Engine Capability Surface</div>
        <div className="capability-list">
          {status.capabilities.map((capability) => (
            <span className="badge badge-neutral" key={capability}>
              {capability}
            </span>
          ))}
        </div>
        <dl className="identity-grid">
          <div><dt>Database ID</dt><dd>{status.databaseId ?? "—"}</dd></div>
          <div><dt>Source ID</dt><dd>{status.source ?? "—"}</dd></div>
          <div><dt>Authoritative Epoch</dt><dd>{status.epoch ?? "—"}</dd></div>
          <div><dt>Memory Safety</dt><dd>forbid(unsafe_code)</dd></div>
        </dl>
      </div>

      <Notice>
        WAL retirement is enabled and guarded by SegmentManifest (`segments/manifest.bin`). Sealed frames are retired into immutable `.tsf` segments while preserving active reference holds.
      </Notice>
    </section>
  );
}

/* ==========================================================================
   Navicat-Style Connection Properties Modal Dialog
   ========================================================================== */

interface ConnectionPropertiesModalProps {
  connection: ConnectionProfile | null;
  isOpen: boolean;
  onClose: () => void;
  onSave: (saved: ConnectionProfile) => void;
}

function ConnectionPropertiesModal({
  connection,
  isOpen,
  onClose,
  onSave,
}: ConnectionPropertiesModalProps) {
  if (!isOpen || !connection) return null;

  const [form, setForm] = useState<ConnectionProfile>({ ...connection });
  const [showToken, setShowToken] = useState(false);
  const [activeTab, setActiveTab] = useState<"general" | "network" | "ssl">("general");
  const [testResult, setTestResult] = useState<{
    success: boolean;
    message: string;
    latencyMs?: number;
    serverVersion?: string;
  } | null>(null);
  const [testing, setTesting] = useState(false);

  useEffect(() => {
    setForm({ ...connection });
    setTestResult(null);
  }, [connection]);

  const handleTest = async () => {
    setTesting(true);
    setTestResult(null);
    try {
      const res = await testConnection(form);
      setTestResult({
        success: res.success,
        message: res.message,
        latencyMs: res.latencyMs,
        serverVersion: res.version,
      });
    } catch (err) {
      setTestResult({
        success: false,
        message: errorText(err),
      });
    } finally {
      setTesting(false);
    }
  };

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    const updated: ConnectionProfile = {
      ...form,
      id: form.id || `conn-${Date.now()}`,
      latencyMs: testResult?.latencyMs ?? form.latencyMs ?? 0.38,
      serverVersion: testResult?.serverVersion ?? form.serverVersion ?? "TNP v1 (temniond 0.1.0)",
    };
    onSave(updated);
  };

  return (
    <div className="connection-modal-overlay" onClick={onClose}>
      <div className="connection-modal" onClick={(e) => e.stopPropagation()}>
        <div className="connection-modal-header">
          <div className="connection-modal-title">
            <span>⚙</span>
            <span>Connection Properties — {form.name || "New Connection"}</span>
          </div>
          <button className="tab-close" type="button" onClick={onClose} style={{ fontSize: "16px" }}>
            ✕
          </button>
        </div>

        <div className="connection-modal-tabs">
          <button
            className={`connection-modal-tab-btn ${activeTab === "general" ? "active" : ""}`}
            onClick={() => setActiveTab("general")}
            type="button"
          >
            General
          </button>
          <button
            className={`connection-modal-tab-btn ${activeTab === "network" ? "active" : ""}`}
            onClick={() => setActiveTab("network")}
            type="button"
          >
            Network & Ports
          </button>
          <button
            className={`connection-modal-tab-btn ${activeTab === "ssl" ? "active" : ""}`}
            onClick={() => setActiveTab("ssl")}
            type="button"
          >
            Security & TLS
          </button>
        </div>

        <form onSubmit={handleSubmit}>
          <div className="connection-modal-body">
            {activeTab === "general" && (
              <div className="form-grid">
                <label className="form-group" style={{ gridColumn: "span 2" }}>
                  Connection Name
                  <input
                    className="text-input"
                    value={form.name}
                    onChange={(e) => setForm({ ...form, name: e.target.value })}
                    placeholder="Local Primary Node (TNP)"
                    required
                  />
                </label>
                <label className="form-group">
                  Host / IP Address
                  <input
                    className="text-input"
                    value={form.host}
                    onChange={(e) => setForm({ ...form, host: e.target.value })}
                    placeholder="127.0.0.1"
                    required
                  />
                </label>
                <label className="form-group">
                  Initial Database
                  <input
                    className="text-input"
                    value={form.database}
                    onChange={(e) => setForm({ ...form, database: e.target.value })}
                    placeholder="temnion_default"
                    required
                  />
                </label>
                <label className="form-group">
                  Username
                  <input
                    className="text-input"
                    value={form.username}
                    onChange={(e) => setForm({ ...form, username: e.target.value })}
                    placeholder="temnion_admin"
                    required
                  />
                </label>
                <label className="form-group">
                  Password / Auth Token
                  <div style={{ display: "flex", gap: "6px" }}>
                    <input
                      className="text-input"
                      type={showToken ? "text" : "password"}
                      value={form.authToken ?? ""}
                      onChange={(e) => setForm({ ...form, authToken: e.target.value })}
                      placeholder="••••••••"
                      style={{ flex: 1 }}
                    />
                    <button
                      className="btn btn-secondary btn-sm"
                      type="button"
                      onClick={() => setShowToken(!showToken)}
                    >
                      {showToken ? "Hide" : "Show"}
                    </button>
                  </div>
                </label>
              </div>
            )}

            {activeTab === "network" && (
              <div className="form-grid">
                <label className="form-group">
                  TNP Protocol Port (Binary Framing)
                  <input
                    className="num-input"
                    type="number"
                    value={form.tnpPort}
                    onChange={(e) => setForm({ ...form, tnpPort: Number(e.target.value) })}
                    placeholder="9180"
                    required
                  />
                </label>
                <label className="form-group">
                  Arrow Flight SQL Port (Columnar)
                  <input
                    className="num-input"
                    type="number"
                    value={form.flightPort}
                    onChange={(e) => setForm({ ...form, flightPort: Number(e.target.value) })}
                    placeholder="9181"
                    required
                  />
                </label>
                <div style={{ gridColumn: "span 2", fontSize: "12px", color: "#94a3b8" }}>
                  <div>TNP Default: <code>9180</code> (Low-latency bi-temporal commands & ingestion)</div>
                  <div>Flight SQL Default: <code>9181</code> (Arrow RecordBatch columnar streaming)</div>
                </div>
              </div>
            )}

            {activeTab === "ssl" && (
              <div style={{ display: "flex", flexDirection: "column", gap: "12px" }}>
                <label style={{ display: "flex", alignItems: "center", gap: "10px", cursor: "pointer" }}>
                  <input
                    type="checkbox"
                    checked={form.tls}
                    onChange={(e) => setForm({ ...form, tls: e.target.checked })}
                  />
                  <span style={{ fontSize: "13px", color: "#e2e8f0" }}>Require TLS / SSL Encryption</span>
                </label>
                <div style={{ fontSize: "12px", color: "#94a3b8", lineHeight: 1.6 }}>
                  When TLS is enabled, the connection enforces TLS 1.3 encryption for both TNP and Arrow Flight streams.
                </div>
              </div>
            )}

            {testResult && (
              <div className={`connection-test-result ${testResult.success ? "success" : "error"}`}>
                <span>{testResult.success ? "✓" : "⚠"}</span>
                <div>
                  <div style={{ fontWeight: 600 }}>{testResult.message}</div>
                  {testResult.latencyMs !== undefined && (
                    <div style={{ fontSize: "11px", opacity: 0.9 }}>
                      Round-trip latency: <strong>{testResult.latencyMs} ms</strong> · {testResult.serverVersion}
                    </div>
                  )}
                </div>
              </div>
            )}
          </div>

          <div className="connection-modal-footer">
            <button
              className="btn btn-secondary btn-sm"
              type="button"
              disabled={testing}
              onClick={handleTest}
            >
              {testing ? "Testing Ping..." : "Test Connection"}
            </button>
            <div style={{ display: "flex", gap: "10px" }}>
              <button className="btn btn-secondary btn-sm" type="button" onClick={onClose}>
                Cancel
              </button>
              <button className="btn btn-primary btn-sm" type="submit">
                Save Connection
              </button>
            </div>
          </div>
        </form>
      </div>
    </div>
  );
}

/* ==========================================================================
   Navicat Action Ribbon Toolbar
   ========================================================================== */

function NavicatRibbon({
  onNewConnection,
  onEditActiveConnection,
  onNewQuery,
  onOpenTables,
  onOpenTemporalPlane,
  onOpenBranches,
  onOpenIngest,
  onOpenStorage,
  onOpenGuide,
  onRefresh,
}: {
  onNewConnection: () => void;
  onEditActiveConnection: () => void;
  onNewQuery: () => void;
  onOpenTables: () => void;
  onOpenTemporalPlane: () => void;
  onOpenBranches: () => void;
  onOpenIngest: () => void;
  onOpenStorage: () => void;
  onOpenGuide: () => void;
  onRefresh: () => void;
}) {
  return (
    <div className="navicat-ribbon" role="toolbar" aria-label="Navicat Action Ribbon">
      <div className="ribbon-group">
        <button className="ribbon-btn" type="button" onClick={onNewConnection} title="Create a new database connection">
          <span className="ribbon-icon">◎</span>
          <span className="ribbon-label">+ Connection</span>
        </button>
        <button className="ribbon-btn" type="button" onClick={onEditActiveConnection} title="Edit active connection properties">
          <span className="ribbon-icon">⚙</span>
          <span className="ribbon-label">Properties</span>
        </button>
      </div>

      <div className="ribbon-group">
        <button className="ribbon-btn" type="button" onClick={onNewQuery} title="Open new query editor tab">
          <span className="ribbon-icon" style={{ color: "#38bdf8" }}>⚡</span>
          <span className="ribbon-label">New Query</span>
        </button>
        <button className="ribbon-btn" type="button" onClick={onOpenTables} title="Open schema and table designer">
          <span className="ribbon-icon" style={{ color: "#a5b4fc" }}>⊞</span>
          <span className="ribbon-label">Table</span>
        </button>
        <button className="ribbon-btn" type="button" onClick={onOpenTemporalPlane} title="Open bi-temporal plane visualizer">
          <span className="ribbon-icon" style={{ color: "#34d399" }}>◴</span>
          <span className="ribbon-label">Time Travel</span>
        </button>
        <button className="ribbon-btn" type="button" onClick={onOpenBranches} title="Open branch and causality DAG">
          <span className="ribbon-icon" style={{ color: "#c084fc" }}>⑂</span>
          <span className="ribbon-label">Branches</span>
        </button>
      </div>

      <div className="ribbon-group">
        <button className="ribbon-btn" type="button" onClick={onOpenIngest} title="Ingest bi-temporal event batches">
          <span className="ribbon-icon">⇧</span>
          <span className="ribbon-label">Ingest</span>
        </button>
        <button className="ribbon-btn" type="button" onClick={onOpenStorage} title="View storage metrics and segment manifests">
          <span className="ribbon-icon">▥</span>
          <span className="ribbon-label">Storage</span>
        </button>
        <button className="ribbon-btn" type="button" onClick={onRefresh} title="Refresh connection and cache">
          <span className="ribbon-icon">⟳</span>
          <span className="ribbon-label">Refresh</span>
        </button>
      </div>

      <div className="ribbon-group">
        <button className="ribbon-btn" type="button" onClick={onOpenGuide} title="Open developer How-To Guide">
          <span className="ribbon-icon" style={{ color: "#f59e0b" }}>📖</span>
          <span className="ribbon-label">Guide</span>
        </button>
      </div>
    </div>
  );
}

/* ==========================================================================
   Navicat Object Explorer Database Tree
   ========================================================================== */

function ObjectExplorerTree({
  connections,
  activeConnId,
  onSelectConnection,
  onEditConnection,
  onDeleteConnection,
  onNewConnection,
  onOpenTable,
  onOpenBranches,
  onOpenStorage,
}: {
  connections: ConnectionProfile[];
  activeConnId: string;
  onSelectConnection: (id: string) => void;
  onEditConnection: (conn: ConnectionProfile) => void;
  onDeleteConnection: (id: string) => void;
  onNewConnection: () => void;
  onOpenTable: (schemaName: string) => void;
  onOpenBranches: () => void;
  onOpenStorage: () => void;
}) {
  const [expandedConns, setExpandedConns] = useState<Record<string, boolean>>({
    "conn-local-primary": true,
  });
  const [expandedSections, setExpandedSections] = useState<Record<string, boolean>>({
    tables: true,
    branches: true,
    storage: false,
  });

  const toggleConn = (id: string) => {
    setExpandedConns((prev) => ({ ...prev, [id]: !prev[id] }));
  };

  const toggleSection = (key: string) => {
    setExpandedSections((prev) => ({ ...prev, [key]: !prev[key] }));
  };

  return (
    <aside className="object-tree-container" aria-label="Database Object Explorer">
      <div className="tree-header">
        <span>Object Explorer</span>
        <div style={{ display: "flex", gap: "6px" }}>
          <button
            className="tree-action-icon-btn"
            title="New Database Connection"
            onClick={onNewConnection}
            type="button"
          >
            +
          </button>
        </div>
      </div>

      <div style={{ padding: "6px 0" }}>
        {connections.map((conn) => {
          const isExpanded = !!expandedConns[conn.id];
          const isActive = conn.id === activeConnId;

          return (
            <div className="tree-node-root" key={conn.id}>
              <div
                className={`tree-node-conn ${isActive ? "active" : ""}`}
                onClick={() => toggleConn(conn.id)}
                title={`temnion://${conn.username}@${conn.host}:${conn.tnpPort}/${conn.database}`}
              >
                <div className="tree-conn-left">
                  <span style={{ fontSize: "10px", color: "#94a3b8", width: "12px" }}>
                    {isExpanded ? "▼" : "▶"}
                  </span>
                  <span
                    className={`tree-status-dot ${isActive ? "connected" : "disconnected"}`}
                    title={isActive ? "Connected" : "Disconnected"}
                  />
                  <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", maxWidth: "155px" }}>
                    {conn.name}
                  </span>
                </div>
                <div className="tree-conn-actions" onClick={(e) => e.stopPropagation()}>
                  <button
                    className="tree-action-icon-btn"
                    title="Edit Connection Properties"
                    onClick={() => onEditConnection(conn)}
                    type="button"
                  >
                    ⚙
                  </button>
                  {!isActive && (
                    <button
                      className="tree-action-icon-btn"
                      title="Connect / Make Active"
                      onClick={() => onSelectConnection(conn.id)}
                      type="button"
                    >
                      ⚡
                    </button>
                  )}
                  {conn.id !== "conn-local-primary" && (
                    <button
                      className="tree-action-icon-btn"
                      title="Delete Connection"
                      onClick={() => onDeleteConnection(conn.id)}
                      type="button"
                    >
                      🗑
                    </button>
                  )}
                </div>
              </div>

              {isExpanded && (
                <div className="tree-children">
                  {/* Database Node */}
                  <div className="tree-sub-folder" style={{ color: "#c7d2fe", fontWeight: 600 }}>
                    <span>🗄️</span>
                    <span>{conn.database}</span>
                  </div>

                  {/* Tables Node */}
                  <div style={{ paddingLeft: "12px" }}>
                    <div
                      className="tree-sub-folder"
                      onClick={() => toggleSection(`${conn.id}-tables`)}
                    >
                      <span style={{ fontSize: "9px" }}>
                        {expandedSections[`${conn.id}-tables`] !== false ? "▼" : "▶"}
                      </span>
                      <span>📁</span>
                      <span>Tables</span>
                      <span className="tree-count-badge">4</span>
                    </div>

                    {expandedSections[`${conn.id}-tables`] !== false && (
                      <div>
                        <div
                          className="tree-leaf-item"
                          onClick={() => onOpenTable("SensorTelemetry")}
                        >
                          <span>⊞</span>
                          <span>SensorTelemetry</span>
                        </div>
                        <div
                          className="tree-leaf-item"
                          onClick={() => onOpenTable("FinancialLedger")}
                        >
                          <span>⊞</span>
                          <span>FinancialLedger</span>
                        </div>
                        <div
                          className="tree-leaf-item"
                          onClick={() => onOpenTable("SystemSecurityAudit")}
                        >
                          <span>⊞</span>
                          <span>SystemSecurityAudit</span>
                        </div>
                        <div
                          className="tree-leaf-item"
                          onClick={() => onOpenTable("StateSnapshot")}
                        >
                          <span>⊞</span>
                          <span>StateSnapshot</span>
                        </div>
                      </div>
                    )}

                    {/* Branches Node */}
                    <div
                      className="tree-sub-folder"
                      onClick={() => toggleSection(`${conn.id}-branches`)}
                    >
                      <span style={{ fontSize: "9px" }}>
                        {expandedSections[`${conn.id}-branches`] !== false ? "▼" : "▶"}
                      </span>
                      <span>📁</span>
                      <span>Branches & DAG</span>
                      <span className="tree-count-badge">3</span>
                    </div>

                    {expandedSections[`${conn.id}-branches`] !== false && (
                      <div>
                        <div className="tree-leaf-item" onClick={onOpenBranches}>
                          <span style={{ color: "#34d399" }}>●</span>
                          <span style={{ color: "#a7f3d0", fontWeight: 600 }}>main (active)</span>
                        </div>
                        <div className="tree-leaf-item" onClick={onOpenBranches}>
                          <span style={{ color: "#c084fc" }}>⑂</span>
                          <span>staging</span>
                        </div>
                        <div className="tree-leaf-item" onClick={onOpenBranches}>
                          <span style={{ color: "#c084fc" }}>⑂</span>
                          <span>hotfix-causal</span>
                        </div>
                      </div>
                    )}

                    {/* Storage & Engine Node */}
                    <div
                      className="tree-sub-folder"
                      onClick={() => toggleSection(`${conn.id}-storage`)}
                    >
                      <span style={{ fontSize: "9px" }}>
                        {expandedSections[`${conn.id}-storage`] ? "▼" : "▶"}
                      </span>
                      <span>📁</span>
                      <span>Storage & Engine</span>
                    </div>

                    {expandedSections[`${conn.id}-storage`] && (
                      <div>
                        <div className="tree-leaf-item" onClick={onOpenStorage}>
                          <span>📄</span>
                          <span>Active WAL Prefix</span>
                        </div>
                        <div className="tree-leaf-item" onClick={onOpenStorage}>
                          <span>📄</span>
                          <span>SegmentManifest</span>
                        </div>
                        <div className="tree-leaf-item" onClick={onOpenStorage}>
                          <span>📄</span>
                          <span>TSF Segments</span>
                        </div>
                      </div>
                    )}
                  </div>
                </div>
              )}
            </div>
          );
        })}
      </div>
    </aside>
  );
}

/* ==========================================================================
   Navicat Multi-Document Tabs Bar
   ========================================================================== */

interface StudioTab {
  id: string;
  type: View | "table";
  title: string;
  icon: string;
  tableName?: string;
  querySnippet?: string;
}

function NavicatTabsBar({
  tabs,
  activeTabId,
  onSelectTab,
  onCloseTab,
  onNewQueryTab,
}: {
  tabs: StudioTab[];
  activeTabId: string;
  onSelectTab: (id: string) => void;
  onCloseTab: (id: string) => void;
  onNewQueryTab: () => void;
}) {
  return (
    <div className="navicat-tabs-bar" role="tablist" aria-label="Open documents">
      {tabs.map((tab) => {
        const isActive = tab.id === activeTabId;
        return (
          <div
            key={tab.id}
            role="tab"
            aria-selected={isActive}
            className={`navicat-tab ${isActive ? "active" : ""}`}
            onClick={() => onSelectTab(tab.id)}
          >
            <span>{tab.icon}</span>
            <span>{tab.title}</span>
            {tabs.length > 1 && (
              <button
                className="tab-close"
                type="button"
                title="Close tab"
                onClick={(e) => {
                  e.stopPropagation();
                  onCloseTab(tab.id);
                }}
              >
                ✕
              </button>
            )}
          </div>
        );
      })}
      <button
        className="tab-add-btn"
        type="button"
        title="New Query Tab"
        onClick={onNewQueryTab}
      >
        +
      </button>
    </div>
  );
}

/* ==========================================================================
   Table Viewer Panel (Navicat Object Viewer)
   ========================================================================== */

function TableViewerPanel({
  tableName,
  onRunQuery,
}: {
  tableName: string;
  onRunQuery: (query: string) => void;
}) {
  const schemasQuery = useQuery({ queryKey: ["schemas"], queryFn: listSchemas });
  const schemas = schemasQuery.data ?? [];
  const schema = schemas.find((s) => s.name.toLowerCase() === tableName.toLowerCase()) ?? schemas[0];

  const historyQuery = useQuery({ queryKey: ["history", 50], queryFn: () => listHistory(50) });
  const historyRows: EventRow[] = historyQuery.data?.rows ?? [];
  const filteredRows = historyRows.filter(
    (r: EventRow) => (schema ? r.schema === schema.id : true)
  );

  return (
    <section className="view-panel active">
      <div className="table-viewer-toolbar">
        <div style={{ display: "flex", alignItems: "center", gap: "10px" }}>
          <span style={{ fontSize: "18px" }}>⊞</span>
          <div>
            <div style={{ fontSize: "15px", fontWeight: "bold", color: "#f8fafc" }}>
              Table: <span style={{ color: "#38bdf8" }}>{schema?.name ?? tableName}</span>
            </div>
            <div style={{ fontSize: "11px", color: "#94a3b8" }}>
              {schema?.description ?? "Bi-temporal columnar event entity table"}
            </div>
          </div>
        </div>
        <div style={{ display: "flex", gap: "8px" }}>
          <button
            className="btn btn-primary btn-sm"
            type="button"
            onClick={() =>
              onRunQuery(`SELECT entity, schema, valid_time, known_time, sequence\nFROM temnion\nLIMIT 100`)
            }
          >
            ⚡ Open in Query Studio
          </button>
        </div>
      </div>

      {schema && (
        <div className="glass-card" style={{ padding: "14px", marginBottom: "16px" }}>
          <div
            style={{
              fontSize: "12px",
              fontWeight: "bold",
              color: "#cbd5e1",
              textTransform: "uppercase",
              letterSpacing: "0.06em",
              marginBottom: "8px",
            }}
          >
            Column Definitions & Types
          </div>
          <div style={{ display: "flex", flexWrap: "wrap", gap: "8px" }}>
            {schema.fields.map((f) => (
              <span key={f.name} className="stat-pill" style={{ padding: "4px 10px" }}>
                <strong style={{ color: "#e2e8f0" }}>{f.name}</strong>
                <span style={{ color: "#818cf8" }}>: {f.type}</span>
                {f.indexed && <span style={{ color: "#34d399", fontSize: "10px" }}>[IDX]</span>}
              </span>
            ))}
          </div>
        </div>
      )}

      <div className="glass-card" style={{ padding: "16px" }}>
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "12px" }}>
          <div style={{ fontSize: "14px", fontWeight: "600", color: "#e2e8f0" }}>
            Data Grid ({filteredRows.length > 0 ? filteredRows.length : historyRows.length} rows)
          </div>
          <span className="badge badge-neutral">Arrow Columnar Format</span>
        </div>
        <EventTable rows={filteredRows.length > 0 ? filteredRows : historyRows} showPayload={true} />
      </div>
    </section>
  );
}

/* ==========================================================================
   Navicat Bottom Status Bar
   ========================================================================== */

function NavicatStatusBar({
  connection,
  eventCount,
}: {
  connection: ConnectionProfile;
  eventCount: number;
}) {
  return (
    <footer className="navicat-status-bar">
      <div className="status-bar-left">
        <div className="status-bar-item">
          <span className={`tree-status-dot ${connection.status === "connected" ? "connected" : "disconnected"}`} />
          <span>temnion://{connection.username}@{connection.host}:{connection.tnpPort}/{connection.database}</span>
        </div>
        <div className="status-bar-item">
          <span>Latency: <strong style={{ color: "#34d399" }}>{connection.latencyMs ?? 0.38} ms</strong></span>
        </div>
        <div className="status-bar-item">
          <span>Server: {connection.serverVersion ?? "TNP v1 (temniond 0.1.0)"}</span>
        </div>
      </div>
      <div className="status-bar-right">
        <div className="status-bar-item">
          <span>Events: <strong>{eventCount}</strong></span>
        </div>
        <div className="status-bar-item">
          <span>Database: <code>{connection.database}</code></span>
        </div>
        <div className="status-bar-item">
          <span>Branch: <span style={{ color: "#c084fc" }}>main</span></span>
        </div>
        <div className="status-bar-item" style={{ color: "#10b981", fontWeight: 600 }}>
          <span>#![forbid(unsafe_code)]</span>
        </div>
      </div>
    </footer>
  );
}

/* ==========================================================================
   Main Application Root
   ========================================================================== */

export function App() {
  const queryClient = useQueryClient();
  const statusQuery = useQuery({ queryKey: ["engine-status"], queryFn: getEngineStatus, initialData: browserStatus });
  const status = statusQuery.data;

  const connectionsQuery = useQuery({ queryKey: ["connections"], queryFn: listConnections });
  const connections = connectionsQuery.data ?? [
    {
      id: "conn-local-primary",
      name: "Local Primary Node (TNP)",
      host: "127.0.0.1",
      tnpPort: 9180,
      flightPort: 9181,
      database: "temnion_default",
      username: "temnion_admin",
      authToken: "••••••••",
      tls: false,
      lastConnected: "Just now",
      status: "connected" as const,
      latencyMs: 0.38,
      serverVersion: "TNP v1 (temniond 0.1.0)",
    },
  ];

  const [activeConnId, setActiveConnId] = useState<string>("conn-local-primary");
  const activeConnection = useMemo(() => {
    return connections.find((c) => c.id === activeConnId) ?? connections[0];
  }, [connections, activeConnId]);

  const [editingConnection, setEditingConnection] = useState<ConnectionProfile | null>(null);

  const [tabs, setTabs] = useState<StudioTab[]>([
    { id: "tab-guide", type: "guide", title: "How-To Guide", icon: "📖" },
    { id: "tab-query-1", type: "query", title: "Query 1", icon: "⚡" },
  ]);
  const [activeTabId, setActiveTabId] = useState<string>("tab-query-1");
  const [queryCount, setQueryCount] = useState<number>(1);

  const switchMutation = useMutation({
    mutationFn: (id: string) => setActiveConnection(id),
    onSuccess: (active) => {
      setActiveConnId(active.id);
      queryClient.invalidateQueries({ queryKey: ["engine-status"] });
    },
  });

  const saveMutation = useMutation({
    mutationFn: (profile: ConnectionProfile) => saveConnection(profile),
    onSuccess: (updated) => {
      queryClient.setQueryData(["connections"], updated);
      setEditingConnection(null);
    },
  });

  const deleteMutation = useMutation({
    mutationFn: (id: string) => deleteConnection(id),
    onSuccess: (updated) => queryClient.setQueryData(["connections"], updated),
  });

  const openTab = (
    type: View | "table",
    title: string,
    icon: string,
    extra?: { tableName?: string; querySnippet?: string }
  ) => {
    const existing = tabs.find((t) => {
      if (type === "table") return t.type === "table" && t.tableName === extra?.tableName;
      if (type === "query") return false;
      return t.type === type;
    });

    if (existing) {
      setActiveTabId(existing.id);
    } else {
      const newId = `tab-${type}-${Date.now()}`;
      setTabs((prev) => [
        ...prev,
        {
          id: newId,
          type,
          title,
          icon,
          tableName: extra?.tableName,
          querySnippet: extra?.querySnippet,
        },
      ]);
      setActiveTabId(newId);
    }
  };

  const handleNewQueryTab = (snippet?: string) => {
    const nextCount = queryCount + 1;
    setQueryCount(nextCount);
    const newId = `tab-query-${nextCount}`;
    setTabs((prev) => [
      ...prev,
      {
        id: newId,
        type: "query",
        title: `Query ${nextCount}`,
        icon: "⚡",
        querySnippet: snippet,
      },
    ]);
    setActiveTabId(newId);
  };

  const handleCloseTab = (idToClose: string) => {
    if (tabs.length <= 1) return;
    const nextTabs = tabs.filter((t) => t.id !== idToClose);
    setTabs(nextTabs);
    if (activeTabId === idToClose) {
      setActiveTabId(nextTabs[nextTabs.length - 1].id);
    }
  };

  const activeTab = tabs.find((t) => t.id === activeTabId) ?? tabs[0];

  const handleNavigate = (targetView: View, snippet?: string) => {
    if (targetView === "query") {
      handleNewQueryTab(snippet);
    } else {
      const nav = navItems.find((item) => item.id === targetView);
      openTab(targetView, nav ? nav.label : targetView, nav ? nav.icon : "•", { querySnippet: snippet });
    }
  };

  let tabContent: React.ReactNode;
  switch (activeTab.type) {
    case "guide":
      tabContent = <GuidePanel onNavigate={handleNavigate} />;
      break;
    case "query":
      tabContent = (
        <QueryPanel
          connected={status.connected}
          initialQuery={activeTab.querySnippet}
          onNavigateToConnections={() => openTab("connections", "Connections", "◎")}
          onNavigateToGuide={() => openTab("guide", "How-To Guide", "📖")}
        />
      );
      break;
    case "table":
      tabContent = (
        <TableViewerPanel
          tableName={activeTab.tableName ?? "SensorTelemetry"}
          onRunQuery={(q) => handleNewQueryTab(q)}
        />
      );
      break;
    case "history":
      tabContent = <HistoryPanel connected={status.connected} eventCount={status.eventCount} />;
      break;
    case "causality":
      tabContent = <CausalityPanel connected={status.connected} eventCount={status.eventCount} />;
      break;
    case "schemas":
      tabContent = <SchemaPanel />;
      break;
    case "ingest":
      tabContent = <IngestPanel connected={status.connected} />;
      break;
    case "connections":
      tabContent = (
        <ConnectionsPanel
          status={status}
          onEditConnection={(conn) => setEditingConnection(conn)}
        />
      );
      break;
    case "metrics":
      tabContent = <MetricsPanel status={status} />;
      break;
  }

  return (
    <div className="studio-layout">
      <header className="studio-header">
        <Logo />
        <NavicatRibbon
          onNewConnection={() =>
            setEditingConnection({
              id: "",
              name: "New Connection",
              host: "127.0.0.1",
              tnpPort: 9180,
              flightPort: 9181,
              database: "temnion_default",
              username: "temnion_admin",
              authToken: "",
              tls: false,
              status: "disconnected",
              latencyMs: 0.38,
              serverVersion: "TNP v1 (temniond 0.1.0)",
            })
          }
          onEditActiveConnection={() => setEditingConnection(activeConnection)}
          onNewQuery={() => handleNewQueryTab()}
          onOpenTables={() => openTab("schemas", "Schema Catalog", "⊞")}
          onOpenTemporalPlane={() => openTab("history", "Temporal Plane", "◴")}
          onOpenBranches={() => openTab("causality", "Branches & DAG", "⑂")}
          onOpenIngest={() => openTab("ingest", "Ingestion", "⇧")}
          onOpenStorage={() => openTab("metrics", "Storage & Health", "▥")}
          onOpenGuide={() => openTab("guide", "How-To Guide", "📖")}
          onRefresh={() => {
            queryClient.invalidateQueries();
          }}
        />
        <div className="header-status">
          <div
            className={`status-chip ${status.connected ? "connected" : ""}`}
            onClick={() => openTab("connections", "Connections", "◎")}
            style={{ cursor: "pointer" }}
            title="Click to manage database connections"
          >
            <span className="pulse-dot" />
            <span>{status.connected ? `Connected · ${status.eventCount} events` : "No database connected"}</span>
          </div>
          <div className="status-chip branch-chip">main timeline</div>
          <div className="status-chip mode-chip">TNP Port {activeConnection.tnpPort}</div>
        </div>
      </header>

      <div className="studio-body">
        <ObjectExplorerTree
          connections={connections}
          activeConnId={activeConnId}
          onSelectConnection={(id) => switchMutation.mutate(id)}
          onEditConnection={(conn) => setEditingConnection(conn)}
          onDeleteConnection={(id) => deleteMutation.mutate(id)}
          onNewConnection={() =>
            setEditingConnection({
              id: "",
              name: "New Connection",
              host: "127.0.0.1",
              tnpPort: 9180,
              flightPort: 9181,
              database: "temnion_default",
              username: "temnion_admin",
              authToken: "",
              tls: false,
              status: "disconnected",
              latencyMs: 0.38,
              serverVersion: "TNP v1 (temniond 0.1.0)",
            })
          }
          onOpenTable={(tableName) => openTab("table", `Table: ${tableName}`, "⊞", { tableName })}
          onOpenBranches={() => openTab("causality", "Branches & DAG", "⑂")}
          onOpenStorage={() => openTab("metrics", "Storage & Health", "▥")}
        />

        <div className="studio-main-workspace">
          <NavicatTabsBar
            tabs={tabs}
            activeTabId={activeTabId}
            onSelectTab={(id) => setActiveTabId(id)}
            onCloseTab={handleCloseTab}
            onNewQueryTab={() => handleNewQueryTab()}
          />

          <main className="studio-content">{tabContent}</main>

          <NavicatStatusBar connection={activeConnection} eventCount={status.eventCount} />
        </div>
      </div>

      {editingConnection && (
        <ConnectionPropertiesModal
          connection={editingConnection}
          isOpen={true}
          onClose={() => setEditingConnection(null)}
          onSave={(updated) => saveMutation.mutate(updated)}
        />
      )}
    </div>
  );
}
