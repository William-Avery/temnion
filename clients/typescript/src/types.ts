// SPDX-License-Identifier: AGPL-3.0-only

export enum QueryFormat {
  Temql = 1,
  CompactTem = 2,
  Sql = 3,
}

export interface QueryRow {
  shard: number;
  slot: number;
  generation: number;
  validTime: bigint;
  knownTime: bigint;
  sequence: bigint;
  entityId: string;
}

export interface QueryResult {
  rowsCount: number;
  eventsScanned: number;
  truncated: boolean;
  rows: QueryRow[];
}

export interface LiveEvent {
  subscriptionId: bigint;
  sequence: bigint;
  isLive: boolean;
  shard: number;
  slot: number;
  generation: number;
  entityId: string;
  schema: number;
  validClock: number;
  validTime: bigint;
  knownClock: number;
  knownTime: bigint;
  payloadHex: string;
  payloadBytes: Uint8Array;
}

export interface ServerDescription {
  serverId: string;
  version: number;
  capabilities: string[];
}

export interface ConnectionOptions {
  host?: string;
  port?: number;
  database?: string;
  username?: string;
  authToken?: string;
  timeoutMs?: number;
}

export class TemnionError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "TemnionError";
  }
}

export class ConnectionError extends TemnionError {
  constructor(message: string) {
    super(message);
    this.name = "ConnectionError";
  }
}

export class ProtocolError extends TemnionError {
  constructor(message: string) {
    super(message);
    this.name = "ProtocolError";
  }
}

export class QueryError extends TemnionError {
  constructor(message: string) {
    super(message);
    this.name = "QueryError";
  }
}

export class SubscriptionError extends TemnionError {
  constructor(message: string) {
    super(message);
    this.name = "SubscriptionError";
  }
}
