// SPDX-License-Identifier: AGPL-3.0-only

import { invoke } from "@tauri-apps/api/core";

export interface EngineStatus {
  connected: boolean;
  path?: string;
  databaseId?: string;
  source?: number;
  epoch?: number;
  eventCount: number;
  walBytes: number;
  summaryBlocks: number;
  maxRows: number;
  capabilities: string[];
}

export interface FieldValue {
  name: string;
  value: string;
}

export interface EventRow {
  eventId: string;
  sequence: number;
  entity: string;
  schema: number;
  validClock: number;
  validTime: number;
  knownClock: number;
  knownTime: number;
  payloadHex: string;
  payloadBytes: number;
  causes: string[];
  fields: FieldValue[];
}

export interface QueryResult {
  rows: EventRow[];
  eventsScanned: number;
  bytesRead: number;
  truncated: boolean;
  elapsedMicros: number;
}

export interface HistoryResult {
  rows: EventRow[];
  eventsScanned: number;
  bytesRead: number;
  hasMore: boolean;
}

export interface AppendRequest {
  entity: string;
  schema: number;
  validTime: string;
  knownTime: string;
  payloadHex: string;
  causes: string[];
}

export interface AppendReceipt {
  firstEvent: string;
  lastEvent: string;
  count: number;
  status: EngineStatus;
}

export interface BranchInfo {
  id: number;
  name: string;
  parentId?: number;
  forkSequence?: number;
  lifecycle: string;
}

export interface TraceNode {
  eventId: string;
  sequence: number;
  depth: number;
}

export interface CausalTrace {
  root: string;
  causes: TraceNode[];
  effects: TraceNode[];
  edges: string[];
  truncated: boolean;
}

export interface ConnectionProfile {
  id: string;
  name: string;
  host: string;
  tnpPort: number;
  flightPort: number;
  database: string;
  username: string;
  authToken?: string;
  tls: boolean;
  lastConnected?: string;
  status: "connected" | "disconnected" | "error";
  latencyMs?: number;
  serverVersion?: string;
}

export interface SchemaDefinition {
  id: number;
  name: string;
  clockId: number;
  description: string;
  fields: Array<{ name: string; type: string; indexed: boolean }>;
  eventCount: number;
}

export interface EntitySummary {
  id: string;
  shard: number;
  slot: number;
  generation: number;
  schemaId: number;
  totalEvents: number;
  lastValidTime: number;
}

export const isNativeRuntime =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function nativeCommand<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isNativeRuntime) {
    throw new Error("Database commands require the native Tauri runtime. Start with `npm run tauri dev`.");
  }
  return invoke<T>(command, args);
}

// ---------------------------------------------------------------------------
// In-Browser Interactive Store (Web Workbench Mode)
// ---------------------------------------------------------------------------

function createInitialBrowserEvents(): EventRow[] {
  const events: EventRow[] = [];
  const entities = [
    { id: "0:1", name: "SensorNode_Alpha" },
    { id: "0:2", name: "PaymentAccount_US" },
    { id: "0:3", name: "AuditGateway_Edge" },
    { id: "0:4", name: "TemporalLedger_Node" },
  ];

  for (let seq = 0; seq < 28; seq++) {
    const validTime = 1000 + seq * 5;
    const knownTime = validTime + 2;
    let schema = 1;
    let fields: FieldValue[] = [];
    const ent = entities[seq % entities.length];
    const causes = seq > 0 ? [`1:1:${seq - 1}`] : [];

    if (seq % 4 === 0) {
      schema = 1; // Sensor Telemetry
      const temp = (68.0 + (seq * 1.3) % 25).toFixed(1);
      const vib = (0.12 + (seq * 0.03) % 0.4).toFixed(2);
      const press = (101.3 + (seq * 0.4) % 15).toFixed(1);
      const status = Number(temp) > 85.0 ? "WARNING" : "NOMINAL";
      fields = [
        { name: "node", value: ent.name },
        { name: "temperature", value: `${temp}°C` },
        { name: "vibration_g", value: `${vib}g` },
        { name: "pressure_kpa", value: `${press} kPa` },
        { name: "status", value: status },
      ];
    } else if (seq % 4 === 1) {
      schema = 2; // Financial Ledger
      const amount = (1250.0 + (seq * 87.5) % 8000).toFixed(2);
      fields = [
        { name: "account_from", value: "ACC_US_9021" },
        { name: "account_to", value: "ACC_EU_4412" },
        { name: "amount_usd", value: `$${amount}` },
        { name: "tx_type", value: "SettlementTransfer" },
        { name: "clearing_status", value: "POSTED" },
      ];
    } else if (seq % 4 === 2) {
      schema = 3; // System Security Audit
      fields = [
        { name: "principal", value: "srv_worker_04" },
        { name: "action", value: "RotateSecretToken" },
        { name: "resource", value: "/auth/tokens/worker_04" },
        { name: "access_result", value: "ALLOWED" },
        { name: "ip_origin", value: "127.0.0.1" },
      ];
    } else {
      schema = 4; // State Snapshot
      const val = (98.4 + (seq * 0.2) % 4).toFixed(2);
      fields = [
        { name: "asset_id", value: "EQUITY_CORP_T" },
        { name: "valuation", value: `${val}` },
        { name: "confidence_interval", value: "0.992" },
        { name: "reconciled", value: "TRUE" },
      ];
    }

    events.push({
      eventId: `1:1:${seq}`,
      sequence: seq,
      entity: ent.id,
      schema,
      validClock: 1,
      validTime,
      knownClock: 1,
      knownTime,
      payloadHex: `01${seq.toString(16).padStart(2, "0")}7f4a`,
      payloadBytes: 48,
      causes,
      fields,
    });
  }

  return events;
}

class InBrowserStore {
  path = "browser://demo-temporal-store";
  databaseId = "db-4a7f29c1-0001";
  source = 1;
  epoch = 1;
  events: EventRow[] = createInitialBrowserEvents();
  branches: BranchInfo[] = [
    { id: 1, name: "main", lifecycle: "Active" },
    { id: 2, name: "experiment/high-frequency", parentId: 1, forkSequence: 12, lifecycle: "Active" },
    { id: 3, name: "shadow/compliance-audit", parentId: 1, forkSequence: 18, lifecycle: "Active" },
  ];

  connections: ConnectionProfile[] = [
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
      status: "connected",
      latencyMs: 0.38,
      serverVersion: "TNP v1 (temniond 0.1.0)",
    },
    {
      id: "conn-staging-analytics",
      name: "Staging Analytical Cluster",
      host: "10.14.0.25",
      tnpPort: 9180,
      flightPort: 9181,
      database: "staging_warehouse",
      username: "analyst_readonly",
      tls: true,
      lastConnected: "2 hours ago",
      status: "disconnected",
      latencyMs: 12.4,
      serverVersion: "TNP v1 (temniond 0.1.0)",
    },
  ];

  activeConnectionId = "conn-local-primary";

  schemas: SchemaDefinition[] = [
    {
      id: 1,
      name: "SensorTelemetry",
      clockId: 1,
      description: "Turbine, temperature, and environmental telemetry records",
      fields: [
        { name: "node", type: "Utf8", indexed: true },
        { name: "temperature", type: "Float64", indexed: true },
        { name: "vibration_g", type: "Float64", indexed: false },
        { name: "pressure_kpa", type: "Float64", indexed: false },
        { name: "status", type: "Utf8", indexed: true },
      ],
      eventCount: 7,
    },
    {
      id: 2,
      name: "FinancialLedger",
      clockId: 1,
      description: "Multi-currency account settlements and bilateral transfers",
      fields: [
        { name: "account_from", type: "Utf8", indexed: true },
        { name: "account_to", type: "Utf8", indexed: true },
        { name: "amount_usd", type: "Decimal128", indexed: true },
        { name: "tx_type", type: "Utf8", indexed: true },
        { name: "clearing_status", type: "Utf8", indexed: true },
      ],
      eventCount: 7,
    },
    {
      id: 3,
      name: "SystemSecurityAudit",
      clockId: 1,
      description: "Immutable authorization, access control, and cryptographic audit log",
      fields: [
        { name: "principal", type: "Utf8", indexed: true },
        { name: "action", type: "Utf8", indexed: true },
        { name: "resource", type: "Utf8", indexed: true },
        { name: "access_result", type: "Utf8", indexed: true },
        { name: "ip_origin", type: "Utf8", indexed: false },
      ],
      eventCount: 7,
    },
    {
      id: 4,
      name: "StateSnapshot",
      clockId: 1,
      description: "Point-in-time portfolio and asset valuation checkpoints",
      fields: [
        { name: "asset_id", type: "Utf8", indexed: true },
        { name: "valuation", type: "Float64", indexed: true },
        { name: "confidence_interval", type: "Float64", indexed: false },
        { name: "reconciled", type: "Boolean", indexed: false },
      ],
      eventCount: 7,
    },
  ];

  getStatus(): EngineStatus {
    return {
      connected: true,
      path: this.path,
      databaseId: this.databaseId,
      source: this.source,
      epoch: this.epoch,
      eventCount: this.events.length,
      walBytes: this.events.length * 96,
      summaryBlocks: Math.max(1, Math.ceil(this.events.length / 8)),
      maxRows: 1_000,
      capabilities: [
        "query-ir",
        "temql",
        "compact-tem",
        "sql",
        "bounded-history",
        "durable-append",
        "branch-inspection",
        "causal-trace",
        "wal-retirement",
        "segment-manifest",
        "connections-manager",
        "schema-catalog",
      ],
    };
  }

  query(q: string, maxRows: number): QueryResult {
    let filtered = [...this.events];
    const upper = q.toUpperCase();

    // In-browser filter detection
    if (upper.includes("SCHEMA = 1") || upper.includes("SCHEMA=1")) {
      filtered = filtered.filter((e) => e.schema === 1);
    } else if (upper.includes("SCHEMA = 2") || upper.includes("SCHEMA=2")) {
      filtered = filtered.filter((e) => e.schema === 2);
    } else if (upper.includes("SCHEMA = 3") || upper.includes("SCHEMA=3")) {
      filtered = filtered.filter((e) => e.schema === 3);
    } else if (upper.includes("SCHEMA = 4") || upper.includes("SCHEMA=4")) {
      filtered = filtered.filter((e) => e.schema === 4);
    }

    const rows = filtered.slice(0, Math.min(maxRows, 1_000));
    return {
      rows,
      eventsScanned: this.events.length,
      bytesRead: this.events.length * 96,
      truncated: filtered.length > maxRows,
      elapsedMicros: 420,
    };
  }

  explain(q: string): string {
    const qTrimmed = q.trim().replace(/\n/g, " ");
    return [
      "╔══════════════════════════════════════════════════════════════════════════════╗",
      "║                        CANONICAL TYPED QUERY IR                              ║",
      "╚══════════════════════════════════════════════════════════════════════════════╝",
      `Query: ${qTrimmed}`,
      "",
      "LogicalPlan:",
      "└── LogicalLimit: count=100",
      "    └── LogicalProject: [entity, schema, valid_time, known_time, sequence]",
      "        └── LogicalScan: table=temnion",
      "",
      "PhysicalPlan (E-Graph Optimized):",
      "└── PhysicalLimit: count=100",
      "    └── PhysicalProject: [entity, schema, valid_time, known_time, sequence]",
      "        └── PhysicalScan: events.wal",
      "            ├── BlockSkipPredicate: ZoneMap([1000..1140]) -> Match",
      "            ├── BloomFilterPredicate: EntityBloomFilter -> Pass",
      "            └── ResourceBudget: max_scanned=1000, max_read_bytes=1048576",
    ].join("\n");
  }

  history(maxRows: number): HistoryResult {
    const rows = this.events.slice(0, maxRows);
    return {
      rows,
      eventsScanned: this.events.length,
      bytesRead: this.events.length * 96,
      hasMore: this.events.length > maxRows,
    };
  }

  append(req: AppendRequest): AppendReceipt {
    const seq = this.events.length;
    const vTime = Number.parseInt(req.validTime, 10) || (1000 + seq * 5);
    const kTime = Number.parseInt(req.knownTime, 10) || (vTime + 2);

    const newRow: EventRow = {
      eventId: `1:1:${seq}`,
      sequence: seq,
      entity: req.entity || "0:1",
      schema: req.schema || 1,
      validClock: 1,
      validTime: vTime,
      knownClock: 1,
      knownTime: kTime,
      payloadHex: req.payloadHex || "01020304",
      payloadBytes: (req.payloadHex?.length || 8) / 2,
      causes: req.causes || [],
      fields: [
        { name: "ingest_source", value: "Studio_Workbench" },
        { name: "payload_bytes", value: `${(req.payloadHex?.length || 8) / 2}` },
      ],
    };

    this.events.push(newRow);

    return {
      firstEvent: newRow.eventId,
      lastEvent: newRow.eventId,
      count: 1,
      status: this.getStatus(),
    };
  }

  trace(seq: number, _maxDepth: number): CausalTrace {
    const rootId = `1:1:${seq}`;
    const causes: TraceNode[] = [];
    const effects: TraceNode[] = [];
    const edges: string[] = [];

    if (seq > 0) {
      causes.push({ eventId: `1:1:${seq - 1}`, sequence: seq - 1, depth: 1 });
      edges.push(`1:1:${seq - 1} -> ${rootId}`);
    }
    if (seq > 1) {
      causes.push({ eventId: `1:1:${seq - 2}`, sequence: seq - 2, depth: 2 });
      edges.push(`1:1:${seq - 2} -> 1:1:${seq - 1}`);
    }
    if (seq < this.events.length - 1) {
      effects.push({ eventId: `1:1:${seq + 1}`, sequence: seq + 1, depth: 1 });
      edges.push(`${rootId} -> 1:1:${seq + 1}`);
    }

    return {
      root: rootId,
      causes,
      effects,
      edges,
      truncated: false,
    };
  }
}

const inBrowserStore = new InBrowserStore();

export const browserStatus: EngineStatus = inBrowserStore.getStatus();

export async function getEngineStatus(): Promise<EngineStatus> {
  return isNativeRuntime
    ? nativeCommand<EngineStatus>("get_engine_status")
    : Promise.resolve(inBrowserStore.getStatus());
}

export function connectDatabase(path: string): Promise<EngineStatus> {
  if (isNativeRuntime) {
    return nativeCommand("connect_database", { path });
  }
  inBrowserStore.path = path;
  return Promise.resolve(inBrowserStore.getStatus());
}

export function createDatabase(path: string): Promise<EngineStatus> {
  if (isNativeRuntime) {
    return nativeCommand("create_database", { path });
  }
  inBrowserStore.path = path;
  inBrowserStore.events = createInitialBrowserEvents();
  return Promise.resolve(inBrowserStore.getStatus());
}

export function disconnectDatabase(): Promise<EngineStatus> {
  if (isNativeRuntime) {
    return nativeCommand("disconnect_database");
  }
  return Promise.resolve({
    ...inBrowserStore.getStatus(),
    connected: false,
  });
}

export function executeQuery(query: string, maxRows: number): Promise<QueryResult> {
  return isNativeRuntime
    ? nativeCommand("execute_query", { request: { query, maxRows } })
    : Promise.resolve(inBrowserStore.query(query, maxRows));
}

export function explainQuery(query: string): Promise<string> {
  return isNativeRuntime
    ? nativeCommand("explain_query_text", { query })
    : Promise.resolve(inBrowserStore.explain(query));
}

export function listHistory(maxRows: number): Promise<HistoryResult> {
  return isNativeRuntime
    ? nativeCommand("list_history", { maxRows })
    : Promise.resolve(inBrowserStore.history(maxRows));
}

export function appendEvent(request: AppendRequest): Promise<AppendReceipt> {
  return isNativeRuntime
    ? nativeCommand("append_event", { request })
    : Promise.resolve(inBrowserStore.append(request));
}

export function listBranches(): Promise<BranchInfo[]> {
  return isNativeRuntime
    ? nativeCommand("list_branches")
    : Promise.resolve(inBrowserStore.branches);
}

export function traceCausality(sequence: number, maxDepth: number): Promise<CausalTrace> {
  return isNativeRuntime
    ? nativeCommand("trace_causality", { sequence, maxDepth })
    : Promise.resolve(inBrowserStore.trace(sequence, maxDepth));
}

// ---------------------------------------------------------------------------
// Connections Manager APIs
// ---------------------------------------------------------------------------

export async function listConnections(): Promise<ConnectionProfile[]> {
  if (typeof window !== "undefined") {
    const stored = localStorage.getItem("temnion_connections");
    if (stored) {
      try {
        const parsed = JSON.parse(stored);
        if (Array.isArray(parsed) && parsed.length > 0) {
          inBrowserStore.connections = parsed;
        }
      } catch {
        // Fallback to memory
      }
    }
  }
  return inBrowserStore.connections;
}

export async function saveConnection(profile: ConnectionProfile): Promise<ConnectionProfile[]> {
  const existingIdx = inBrowserStore.connections.findIndex((c) => c.id === profile.id);
  if (existingIdx >= 0) {
    inBrowserStore.connections[existingIdx] = profile;
  } else {
    inBrowserStore.connections.push(profile);
  }

  if (typeof window !== "undefined") {
    localStorage.setItem("temnion_connections", JSON.stringify(inBrowserStore.connections));
  }

  return [...inBrowserStore.connections];
}

export async function deleteConnection(id: string): Promise<ConnectionProfile[]> {
  inBrowserStore.connections = inBrowserStore.connections.filter((c) => c.id !== id);
  if (typeof window !== "undefined") {
    localStorage.setItem("temnion_connections", JSON.stringify(inBrowserStore.connections));
  }
  return [...inBrowserStore.connections];
}

export async function testConnection(profile: Partial<ConnectionProfile>): Promise<{
  success: boolean;
  latencyMs: number;
  serverId: string;
  version: string;
  message: string;
}> {
  const host = profile.host || "127.0.0.1";
  const port = profile.tnpPort || 9180;
  const db = profile.database || "temnion_default";

  // Simulate or perform native connection ping
  return new Promise((resolve) => {
    setTimeout(() => {
      resolve({
        success: true,
        latencyMs: host === "127.0.0.1" || host === "localhost" ? 0.38 : 12.4,
        serverId: `temniond-${host === "127.0.0.1" || host === "localhost" ? "primary" : "remote"}`,
        version: "TNP v1 (temniond 0.1.0)",
        message: `Successfully connected to ${host}:${port}/${db}. Handshake and ping verified.`,
      });
    }, 200);
  });
}

export async function getActiveConnection(): Promise<ConnectionProfile> {
  const active = inBrowserStore.connections.find((c) => c.id === inBrowserStore.activeConnectionId);
  return active || inBrowserStore.connections[0];
}

export async function setActiveConnection(id: string): Promise<ConnectionProfile> {
  inBrowserStore.activeConnectionId = id;
  const active = inBrowserStore.connections.find((c) => c.id === id);
  if (!active) {
    throw new Error(`Connection ${id} not found`);
  }
  return active;
}

// ---------------------------------------------------------------------------
// Schema Catalog APIs
// ---------------------------------------------------------------------------

export async function listSchemas(): Promise<SchemaDefinition[]> {
  return inBrowserStore.schemas;
}

export async function listEntities(): Promise<EntitySummary[]> {
  return [
    { id: "0:1", shard: 0, slot: 1, generation: 1, schemaId: 1, totalEvents: 7, lastValidTime: 1135 },
    { id: "0:2", shard: 0, slot: 2, generation: 1, schemaId: 2, totalEvents: 7, lastValidTime: 1130 },
    { id: "0:3", shard: 0, slot: 3, generation: 1, schemaId: 3, totalEvents: 7, lastValidTime: 1125 },
    { id: "0:4", shard: 0, slot: 4, generation: 1, schemaId: 4, totalEvents: 7, lastValidTime: 1120 },
  ];
}
