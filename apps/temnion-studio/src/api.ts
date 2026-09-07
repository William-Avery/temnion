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

export const browserStatus: EngineStatus = {
  connected: false,
  eventCount: 0,
  walBytes: 0,
  summaryBlocks: 0,
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

export const isNativeRuntime =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function nativeCommand<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isNativeRuntime) {
    throw new Error("Database commands require the native Tauri runtime. Start with `npm run tauri dev`.");
  }
  return invoke<T>(command, args);
}

export async function getEngineStatus(): Promise<EngineStatus> {
  return isNativeRuntime
    ? nativeCommand<EngineStatus>("get_engine_status")
    : Promise.resolve(browserStatus);
}

export function connectDatabase(path: string): Promise<EngineStatus> {
  return nativeCommand("connect_database", { path });
}

export function createDatabase(path: string): Promise<EngineStatus> {
  return nativeCommand("create_database", { path });
}

export function disconnectDatabase(): Promise<EngineStatus> {
  return nativeCommand("disconnect_database");
}

export function executeQuery(query: string, maxRows: number): Promise<QueryResult> {
  return nativeCommand("execute_query", { request: { query, maxRows } });
}

export function explainQuery(query: string): Promise<string> {
  return nativeCommand("explain_query_text", { query });
}

export function listHistory(maxRows: number): Promise<HistoryResult> {
  return nativeCommand("list_history", { maxRows });
}

export function appendEvent(request: AppendRequest): Promise<AppendReceipt> {
  return nativeCommand("append_event", { request });
}

export function listBranches(): Promise<BranchInfo[]> {
  return nativeCommand("list_branches");
}

export function traceCausality(sequence: number, maxDepth: number): Promise<CausalTrace> {
  return nativeCommand("trace_causality", { sequence, maxDepth });
}

export function getTzeentchSummary(): Promise<TzeentchSummary> {
  return isNativeRuntime
    ? nativeCommand<TzeentchSummary>("get_tzeentch_summary")
    : Promise.resolve({
        organismId: "ORGX-Prime",
        organs: ["VisualCortex", "MotorExecutive", "WorkingMemory", "WorldModel"],
        cells: ["SensoryCell0", "FeatureAttn1", "PolicyCell2", "PredictionCell3"],
        migrationMode: "ShadowMirror",
        mirrorEnqueued: 128,
        mirrorDrained: 128,
        dropCount: 0,
      });
}

export function getTzeentchCadenceStats(): Promise<TzeentchCadenceStats> {
  return isNativeRuntime
    ? nativeCommand<TzeentchCadenceStats>("get_tzeentch_cadence_stats")
    : Promise.resolve({
        fastHz: 120,
        fastTicks: 12000,
        mediumHz: 20,
        mediumTicks: 2000,
        slowHz: 1,
        slowTicks: 100,
        backgroundHz: 0.1,
        backgroundTicks: 10,
        dropCount: 0,
        queuePressure: 0.01,
      });
}

export function inspectTzeentchActionTrace(sequence: number): Promise<ActionTrace> {
  return isNativeRuntime
    ? nativeCommand<ActionTrace>("inspect_tzeentch_action_trace", { sequence })
    : Promise.resolve({
        actionEventId: "1:1:100",
        nodes: [
          { kind: "percept", label: "Percept (VisualCortex)", detail: "Sensor: Cam0", timestamp: 1000, isGap: false },
          { kind: "cell", label: "Processing (ExecutiveOrgan)", detail: "Cell: PolicyCell2", timestamp: 1005, isGap: false },
          { kind: "belief", label: "Belief (ObstacleAhead)", detail: "Confidence: 0.95", timestamp: 1008, confidence: 0.95, isGap: false },
          { kind: "prediction", label: "Prediction (CollisionRisk)", detail: "Probability: 0.88", timestamp: 1010, confidence: 0.88, isGap: false },
          { kind: "intention", label: "Intention (EvasiveManeuver)", detail: "Policy: PPO_v2", timestamp: 1012, isGap: false },
          { kind: "action", label: "Action (Act_TurnRight)", detail: "Command: SteerAngle(+0.4rad)", timestamp: 1015, isGap: false },
          { kind: "outcome", label: "Outcome Feedback", detail: "Reward: +1.00", timestamp: 1020, isGap: false },
        ],
        edges: ["1:1:96 -> 1:1:97", "1:1:97 -> 1:1:98", "1:1:98 -> 1:1:99", "1:1:99 -> 1:1:100", "1:1:100 -> 1:1:101"],
        futureLeakageDetected: false,
        totalCauses: 6,
      });
}

export function setTzeentchMigrationMode(mode: string): Promise<string> {
  return isNativeRuntime
    ? nativeCommand<string>("set_tzeentch_migration_mode", { mode })
    : Promise.resolve(mode);
}
