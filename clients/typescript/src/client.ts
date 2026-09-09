// SPDX-License-Identifier: AGPL-3.0-only
import { EventEmitter } from "node:events";
import * as net from "node:net";
import {
  decodeHandshakeResponse,
  decodeLiveEvent,
  decodeSubscribeResponse,
  decodeUnsubscribeResponse,
  encodeHandshakeRequest,
  encodeQueryRequest,
  encodeSubscribeRequest,
  encodeUnsubscribeRequest,
  MessageType,
  TNP_HEADER_LEN,
  TNP_OVERHEAD,
  TnpPacket,
} from "./protocol.js";
import {
  ConnectionError,
  ConnectionOptions,
  LiveEvent,
  ProtocolError,
  QueryError,
  QueryFormat,
  QueryResult,
  QueryRow,
  ServerDescription,
  SubscriptionError,
} from "./types.js";

const DEFAULT_HOST = "127.0.0.1";
const DEFAULT_PORT = 9180;
const DEFAULT_DATABASE = "temnion_default";
const DEFAULT_USER = "temnion_admin";
const DEFAULT_TIMEOUT_MS = 10000;

export class LiveSubscription extends EventEmitter {
  public unsubscribed = false;
  private queue: LiveEvent[] = [];
  private waiters: ((value: IteratorResult<LiveEvent>) => void)[] = [];

  constructor(
    private client: TemnionClient,
    public subscriptionId: bigint,
    public snapshotStart: bigint,
    public snapshotEnd: bigint
  ) {
    super();
  }

  handleEvent(event: LiveEvent) {
    this.emit("event", event);
    if (this.waiters.length > 0) {
      const waiter = this.waiters.shift()!;
      waiter({ value: event, done: false });
    } else {
      this.queue.push(event);
    }
  }

  handleError(err: Error) {
    this.emit("error", err);
  }

  handleEnd() {
    this.emit("end");
    while (this.waiters.length > 0) {
      const waiter = this.waiters.shift()!;
      waiter({ value: undefined as any, done: true });
    }
  }

  async *[Symbol.asyncIterator](): AsyncGenerator<LiveEvent, void, unknown> {
    while (!this.unsubscribed) {
      if (this.queue.length > 0) {
        yield this.queue.shift()!;
      } else {
        const item = await new Promise<IteratorResult<LiveEvent>>((resolve) => {
          this.waiters.push(resolve);
        });
        if (item.done) break;
        yield item.value;
      }
    }
  }

  async unsubscribe(): Promise<void> {
    if (this.unsubscribed) return;
    this.unsubscribed = true;
    await this.client._sendUnsubscribe(this.subscriptionId);
    this.handleEnd();
  }
}

export class TemnionClient {
  public host: string;
  public port: number;
  public database: string;
  public username: string;
  public authToken?: string;
  public timeoutMs: number;

  public serverId?: string;
  public negotiatedVersion?: number;
  public capabilityFlags: bigint = 0n;

  private socket: net.Socket | null = null;
  private recvBuf: Buffer = Buffer.alloc(0);
  private activeSubscription: LiveSubscription | null = null;

  // Single-request response resolver for simple request-response operations
  private pendingResolver: ((pkt: TnpPacket) => void) | null = null;
  private pendingRejecter: ((err: Error) => void) | null = null;

  constructor(options: ConnectionOptions = {}) {
    this.host = options.host ?? DEFAULT_HOST;
    this.port = options.port ?? DEFAULT_PORT;
    this.database = options.database ?? DEFAULT_DATABASE;
    this.username = options.username ?? DEFAULT_USER;
    this.authToken = options.authToken;
    this.timeoutMs = options.timeoutMs ?? DEFAULT_TIMEOUT_MS;
  }

  async connect(): Promise<this> {
    if (this.socket) return this;

    await new Promise<void>((resolve, reject) => {
      const sock = new net.Socket();
      sock.setNoDelay(true);

      sock.setTimeout(this.timeoutMs, () => {
        sock.destroy(new ConnectionError(`Connection timed out after ${this.timeoutMs}ms`));
      });

      sock.connect(this.port, this.host, () => {
        this.socket = sock;
        resolve();
      });

      sock.on("error", (err) => {
        if (!this.socket) {
          reject(new ConnectionError(`Failed to connect to ${this.host}:${this.port}: ${err.message}`));
        } else {
          this.handleSocketError(err);
        }
      });

      sock.on("data", (data) => this.onData(data));
      sock.on("close", () => this.handleSocketClose());
    });

    // Send HandshakeRequest
    const reqPayload = encodeHandshakeRequest(`${this.username}:ts-client`, 0x01n);
    const reqPkt = new TnpPacket(MessageType.HandshakeRequest, 1, reqPayload);
    const respPkt = await this.sendAndAwait(reqPkt);

    if (respPkt.messageType !== MessageType.HandshakeResponse) {
      await this.close();
      throw new ProtocolError(`Expected HandshakeResponse, got ${respPkt.messageType}`);
    }

    const { success, negotiatedVersion, serverId, capabilityFlags } = decodeHandshakeResponse(
      respPkt.payload
    );

    if (!success) {
      await this.close();
      throw new ConnectionError(`Handshake rejected by server ${serverId}`);
    }

    this.serverId = serverId;
    this.negotiatedVersion = negotiatedVersion;
    this.capabilityFlags = capabilityFlags;
    return this;
  }

  async ping(): Promise<number> {
    const start = performance.now();
    const pkt = new TnpPacket(MessageType.Ping, 1, Buffer.from("PING"));
    const resp = await this.sendAndAwait(pkt);
    if (resp.messageType !== MessageType.Pong) {
      throw new ProtocolError(`Expected Pong, got ${resp.messageType}`);
    }
    return performance.now() - start;
  }

  async describe(): Promise<ServerDescription> {
    const pkt = new TnpPacket(MessageType.DescribeRequest, 1);
    const resp = await this.sendAndAwait(pkt);
    if (resp.messageType !== MessageType.DescribeResponse) {
      throw new ProtocolError(`Expected DescribeResponse, got ${resp.messageType}`);
    }
    const data = JSON.parse(resp.payload.toString("utf-8"));
    return {
      serverId: data.server_id ?? "",
      version: data.version ?? 1,
      capabilities: data.capabilities ?? [],
    };
  }

  async query(
    queryStr: string,
    options: { format?: QueryFormat; maxRows?: number } = {}
  ): Promise<QueryResult> {
    const format = options.format ?? QueryFormat.Sql;
    const maxRows = options.maxRows ?? 1000;

    const payload = encodeQueryRequest(queryStr, format, maxRows);
    const pkt = new TnpPacket(MessageType.QueryRequest, 1, payload);

    // 1. Send query and await QueryResponse
    const respPkt = await this.sendAndAwait(pkt);
    if (respPkt.messageType === MessageType.ErrorResponse) {
      throw new QueryError(`Query failed: ${respPkt.payload.toString("utf-8")}`);
    }
    if (respPkt.messageType !== MessageType.QueryResponse) {
      throw new ProtocolError(`Expected QueryResponse, got ${respPkt.messageType}`);
    }

    const meta = JSON.parse(respPkt.payload.toString("utf-8"));
    const eventsScanned = meta.scanned ?? 0;
    const truncated = meta.truncated ?? false;

    // 2. Stream records until StreamEnd
    const rows: QueryRow[] = [];
    while (true) {
      const nextPkt = await this.awaitNextPacket();
      if (nextPkt.messageType === MessageType.StreamEnd) {
        break;
      } else if (nextPkt.messageType === MessageType.StreamRecord) {
        const recordStr = nextPkt.payload.toString("utf-8");
        const parts = recordStr.split("|");
        if (parts.length === 4) {
          const [shardStr, slotStr, genStr] = parts[0].split(":");
          const shard = Number.parseInt(shardStr, 10);
          const slot = Number.parseInt(slotStr, 10);
          const generation = Number.parseInt(genStr, 10);
          const validTime = BigInt(parts[1]);
          const knownTime = BigInt(parts[2]);
          const sequence = BigInt(parts[3]);
          rows.push({
            shard,
            slot,
            generation,
            validTime,
            knownTime,
            sequence,
            entityId: `#${shard}:${slot}:${generation}`,
          });
        }
      } else if (nextPkt.messageType === MessageType.ErrorResponse) {
        throw new QueryError(`Query stream error: ${nextPkt.payload.toString("utf-8")}`);
      }
    }

    return {
      rowsCount: rows.length,
      eventsScanned,
      truncated,
      rows,
    };
  }

  async subscribe(
    queryStr: string,
    options: { format?: QueryFormat; fromSequence?: bigint; fromNow?: boolean } = {}
  ): Promise<LiveSubscription> {
    const format = options.format ?? QueryFormat.Sql;
    const fromSequence = options.fromSequence;
    const fromNow = options.fromNow ?? false;

    const payload = encodeSubscribeRequest(queryStr, format, fromSequence, fromNow);
    const pkt = new TnpPacket(MessageType.SubscribeRequest, 1, payload);

    const respPkt = await this.sendAndAwait(pkt);
    if (respPkt.messageType !== MessageType.SubscribeResponse) {
      throw new ProtocolError(`Expected SubscribeResponse, got ${respPkt.messageType}`);
    }

    const { subscriptionId, success, snapshotStart, snapshotEnd, message } =
      decodeSubscribeResponse(respPkt.payload);

    if (!success) {
      throw new SubscriptionError(`Subscription rejected: ${message}`);
    }

    const sub = new LiveSubscription(this, subscriptionId, snapshotStart, snapshotEnd);
    this.activeSubscription = sub;
    return sub;
  }

  async _sendUnsubscribe(subscriptionId: bigint): Promise<void> {
    const payload = encodeUnsubscribeRequest(subscriptionId);
    const pkt = new TnpPacket(MessageType.UnsubscribeRequest, 1, payload);
    const resp = await this.sendAndAwait(pkt);
    if (resp.messageType === MessageType.UnsubscribeResponse) {
      decodeUnsubscribeResponse(resp.payload);
    }
    if (this.activeSubscription?.subscriptionId === subscriptionId) {
      this.activeSubscription = null;
    }
  }

  async close(): Promise<void> {
    if (this.socket) {
      this.socket.destroy();
      this.socket = null;
    }
    this.recvBuf = Buffer.alloc(0);
    if (this.activeSubscription) {
      this.activeSubscription.handleEnd();
      this.activeSubscription = null;
    }
  }

  private sendPacket(packet: TnpPacket) {
    if (!this.socket) {
      throw new ConnectionError("Client is not connected");
    }
    this.socket.write(packet.encode());
  }

  private sendAndAwait(packet: TnpPacket): Promise<TnpPacket> {
    return new Promise((resolve, reject) => {
      this.pendingResolver = resolve;
      this.pendingRejecter = reject;
      try {
        this.sendPacket(packet);
      } catch (err) {
        this.pendingResolver = null;
        this.pendingRejecter = null;
        reject(err);
      }
    });
  }

  private awaitNextPacket(): Promise<TnpPacket> {
    return new Promise((resolve, reject) => {
      this.pendingResolver = resolve;
      this.pendingRejecter = reject;
      this.tryDispatchPacket();
    });
  }

  private onData(chunk: Buffer) {
    this.recvBuf = Buffer.concat([this.recvBuf, chunk]);
    this.tryDispatchPacket();
  }

  private tryDispatchPacket() {
    while (this.recvBuf.length >= TNP_OVERHEAD) {
      const payloadLen = this.recvBuf.readUInt32LE(12);
      const totalLen = TNP_HEADER_LEN + payloadLen + 4;
      if (this.recvBuf.length < totalLen) {
        break;
      }

      const { packet, consumed } = TnpPacket.decode(this.recvBuf);
      this.recvBuf = this.recvBuf.subarray(consumed);

      if (packet.messageType === MessageType.LiveEvent) {
        if (this.activeSubscription) {
          const event = decodeLiveEvent(packet.payload);
          this.activeSubscription.handleEvent(event);
        }
      } else if (this.pendingResolver) {
        const resolve = this.pendingResolver;
        this.pendingResolver = null;
        this.pendingRejecter = null;
        resolve(packet);
      }
    }
  }

  private handleSocketError(err: Error) {
    if (this.pendingRejecter) {
      this.pendingRejecter(err);
      this.pendingResolver = null;
      this.pendingRejecter = null;
    }
    if (this.activeSubscription) {
      this.activeSubscription.handleError(err);
    }
  }

  private handleSocketClose() {
    if (this.pendingRejecter) {
      this.pendingRejecter(new ConnectionError("Socket closed by server"));
      this.pendingResolver = null;
      this.pendingRejecter = null;
    }
    if (this.activeSubscription) {
      this.activeSubscription.handleEnd();
    }
  }
}
