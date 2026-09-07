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
