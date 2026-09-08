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

export interface TzeentchSummary {
  organismId: string;
  organs: string[];
  cells: string[];
  migrationMode: string;
  mirrorEnqueued: number;
  mirrorDrained: number;
  dropCount: number;
}

export interface TzeentchCadenceStats {
  fastHz: number;
  fastTicks: number;
  mediumHz: number;
  mediumTicks: number;
  slowHz: number;
  slowTicks: number;
  backgroundHz: number;
  backgroundTicks: number;
  dropCount: number;
  queuePressure: number;
}

export interface ActionTraceNode {
  kind: string;
  eventId?: string;
  label: string;
  detail: string;
  timestamp: number;
  confidence?: number;
  isGap: boolean;
}

export interface ActionTrace {
  actionEventId: string;
  nodes: ActionTraceNode[];
  edges: string[];
  futureLeakageDetected: boolean;
  totalCauses: number;
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
    { id: "0:1", name: "Turbine_Alpha" },
    { id: "0:2", name: "VisualCortex" },
    { id: "0:3", name: "ExecutiveOrgan" },
    { id: "0:4", name: "MotorEffector" },
  ];

  for (let seq = 0; seq < 24; seq++) {
    const validTime = 1000 + seq * 5;
    const knownTime = validTime + 2;
    let schema = 1;
    let fields: FieldValue[] = [];
    const ent = entities[seq % entities.length];
    const causes = seq > 0 ? [`1:1:${seq - 1}`] : [];

    if (seq % 4 === 0) {
      schema = 1; // Telemetry
      const temp = (68.0 + (seq * 1.3) % 25).toFixed(1);
      const vib = (0.12 + (seq * 0.03) % 0.4).toFixed(2);
      const status = Number(temp) > 85.0 ? "WARNING" : "NOMINAL";
      fields = [
        { name: "node", value: ent.name },
        { name: "temperature", value: `${temp}°C` },
        { name: "vibration_g", value: `${vib}g` },
        { name: "status", value: status },
      ];
    } else if (seq % 4 === 1) {
      schema = 101; // Tzeentch Percept
      fields = [
        { name: "organ", value: "VisualCortex" },
        { name: "sensor_id", value: "Cam0_Front" },
        { name: "cadence", value: "Fast_120Hz" },
        { name: "confidence", value: "0.96" },
      ];
    } else if (seq % 4 === 2) {
      schema = 102; // Tzeentch Intention
      fields = [
        { name: "goal", value: "CourseCorrection" },
        { name: "policy", value: "NeuralPPO_v4" },
        { name: "deliberation", value: "RiskScore=0.74" },
      ];
    } else {
      schema = 103; // Tzeentch Action
      fields = [
        { name: "actuator", value: "GimbalVector" },
        { name: "deflection_rad", value: "+0.18" },
        { name: "thrust_pct", value: "92%" },
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
      payloadBytes: 32,
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
    { id: 2, name: "experiment/fast-reflexes", parentId: 1, forkSequence: 12, lifecycle: "Active" },
    { id: 3, name: "shadow/eval-candidate-v2", parentId: 1, forkSequence: 18, lifecycle: "Active" },
  ];
  migrationMode = "ShadowMirror";

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
        "tzeentch-explorer",
        "causal-action-trace",
        "cadence-scheduling",
      ],
    };
  }

  query(q: string, maxRows: number): QueryResult {
    let filtered = [...this.events];
    const upper = q.toUpperCase();

    // Basic in-browser filter detection
    if (upper.includes("SCHEMA = 1") || upper.includes("SCHEMA=1")) {
      filtered = filtered.filter((e) => e.schema === 1);
    } else if (upper.includes("SCHEMA = 101") || upper.includes("SCHEMA=101")) {
      filtered = filtered.filter((e) => e.schema === 101);
    } else if (upper.includes("SCHEMA = 102") || upper.includes("SCHEMA=102")) {
      filtered = filtered.filter((e) => e.schema === 102);
    } else if (upper.includes("SCHEMA = 103") || upper.includes("SCHEMA=103")) {
      filtered = filtered.filter((e) => e.schema === 103);
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
      "            ├── BlockSkipPredicate: ZoneMap([1000..1120]) -> Match",
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
        { name: "ingest_source", value: "Studio_Web" },
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

export function getTzeentchSummary(): Promise<TzeentchSummary> {
  return isNativeRuntime
    ? nativeCommand<TzeentchSummary>("get_tzeentch_summary")
    : Promise.resolve({
        organismId: "ORGX-Prime",
        organs: ["VisualCortex", "MotorExecutive", "WorkingMemory", "WorldModel"],
        cells: ["SensoryCell0", "FeatureAttn1", "PolicyCell2", "PredictionCell3"],
        migrationMode: inBrowserStore.migrationMode,
        mirrorEnqueued: 256,
        mirrorDrained: 256,
        dropCount: 0,
      });
}

export function getTzeentchCadenceStats(): Promise<TzeentchCadenceStats> {
  return isNativeRuntime
    ? nativeCommand<TzeentchCadenceStats>("get_tzeentch_cadence_stats")
    : Promise.resolve({
        fastHz: 120.2,
        fastTicks: 28400,
        mediumHz: 20.0,
        mediumTicks: 4720,
        slowHz: 1.0,
        slowTicks: 236,
        backgroundHz: 0.1,
        backgroundTicks: 24,
        dropCount: 0,
        queuePressure: 0.02,
      });
}

export function inspectTzeentchActionTrace(sequence: number): Promise<ActionTrace> {
  return isNativeRuntime
    ? nativeCommand<ActionTrace>("inspect_tzeentch_action_trace", { sequence })
    : Promise.resolve({
        actionEventId: `1:1:${sequence}`,
        nodes: [
          { kind: "percept", label: "Percept (VisualCortex)", detail: "Sensor: Cam0_Front (obstacle detected at 4.2m)", timestamp: 1000, isGap: false },
          { kind: "cell", label: "Processing (ExecutiveOrgan)", detail: "Cell: PolicyCell2 (Feature extraction)", timestamp: 1005, isGap: false },
          { kind: "belief", label: "Belief (CollisionRiskHigh)", detail: "Confidence: 0.94", timestamp: 1008, confidence: 0.94, isGap: false },
          { kind: "prediction", label: "Prediction (TrajectoryCrossing)", detail: "Probability: 0.89", timestamp: 1010, confidence: 0.89, isGap: false },
          { kind: "intention", label: "Intention (EvasiveSteerRight)", detail: "Policy: NeuralPPO_v4", timestamp: 1012, isGap: false },
          { kind: "action", label: "Action (Act_SteerVector)", detail: "Command: RudderSteer(+0.35rad)", timestamp: 1015, isGap: false },
          { kind: "outcome", label: "Outcome Feedback", detail: "Reward: +1.25, clearance achieved", timestamp: 1020, isGap: false },
        ],
        edges: [
          `1:1:${sequence - 5} -> 1:1:${sequence - 4}`,
          `1:1:${sequence - 4} -> 1:1:${sequence - 3}`,
          `1:1:${sequence - 3} -> 1:1:${sequence - 2}`,
          `1:1:${sequence - 2} -> 1:1:${sequence - 1}`,
          `1:1:${sequence - 1} -> 1:1:${sequence}`,
        ],
        futureLeakageDetected: false,
        totalCauses: 5,
      });
}

export function setTzeentchMigrationMode(mode: string): Promise<string> {
  if (isNativeRuntime) {
    return nativeCommand<string>("set_tzeentch_migration_mode", { mode });
  }
  inBrowserStore.migrationMode = mode;
  return Promise.resolve(mode);
}
