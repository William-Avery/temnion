// SPDX-License-Identifier: AGPL-3.0-only
import { crc32 } from "./crc32.js";
import { LiveEvent, ProtocolError, QueryFormat } from "./types.js";

export const TNP_MAGIC = Buffer.from("TNPP");
export const TNP_VERSION = 1;
export const TNP_HEADER_LEN = 16;
export const TNP_OVERHEAD = 20;
export const MAX_PACKET_LEN = 16 * 1024 * 1024;

export enum MessageType {
  HandshakeRequest = 0x0001,
  HandshakeResponse = 0x0002,
  DescribeRequest = 0x0003,
  DescribeResponse = 0x0004,
  QueryRequest = 0x0005,
  QueryResponse = 0x0006,
  StreamRecord = 0x0007,
  StreamEnd = 0x0008,
  ErrorResponse = 0x0009,
  Ping = 0x000a,
  Pong = 0x000b,
  SubscribeRequest = 0x000c,
  SubscribeResponse = 0x000d,
  LiveEvent = 0x000e,
  UnsubscribeRequest = 0x000f,
  UnsubscribeResponse = 0x0010,
}

export class TnpPacket {
  constructor(
    public messageType: MessageType,
    public streamId: number = 1,
    public payload: Buffer = Buffer.alloc(0),
    public version: number = TNP_VERSION
  ) {}

  encode(): Buffer {
    if (this.payload.length > MAX_PACKET_LEN) {
      throw new ProtocolError(`Payload length ${this.payload.length} exceeds limit`);
    }

    const totalLen = TNP_HEADER_LEN + this.payload.length + 4;
    const buf = Buffer.alloc(totalLen);

    // Magic: b"TNPP"
    TNP_MAGIC.copy(buf, 0);
    // Version: u16 LE
    buf.writeUInt16LE(this.version, 4);
    // Message Type: u16 LE
    buf.writeUInt16LE(this.messageType, 6);
    // Stream ID: u32 LE
    buf.writeUInt32LE(this.streamId, 8);
    // Payload length: u32 LE
    buf.writeUInt32LE(this.payload.length, 12);
    // Payload
    this.payload.copy(buf, 16);

    // Checksum: CRC32 of header + payload
    const checksum = crc32(buf.subarray(0, TNP_HEADER_LEN + this.payload.length));
    buf.writeUInt32LE(checksum, TNP_HEADER_LEN + this.payload.length);

    return buf;
  }

  static decode(buf: Buffer): { packet: TnpPacket; consumed: number } {
    if (buf.length < TNP_OVERHEAD) {
      throw new ProtocolError("Buffer too short for TNP header");
    }

    if (!buf.subarray(0, 4).equals(TNP_MAGIC)) {
      throw new ProtocolError("Invalid TNP packet magic, expected TNPP");
    }

    const version = buf.readUInt16LE(4);
    if (version !== TNP_VERSION) {
      throw new ProtocolError(`Unsupported TNP version: ${version}`);
    }

    const msgType = buf.readUInt16LE(6) as MessageType;
    const streamId = buf.readUInt32LE(8);
    const payloadLen = buf.readUInt32LE(12);

    if (payloadLen > MAX_PACKET_LEN) {
      throw new ProtocolError(`TNP payload length ${payloadLen} exceeds limit`);
    }

    const totalLen = TNP_HEADER_LEN + payloadLen + 4;
    if (buf.length < totalLen) {
      throw new ProtocolError("Incomplete TNP packet in buffer");
    }

    const expectedChecksum = buf.readUInt32LE(TNP_HEADER_LEN + payloadLen);
    const computedChecksum = crc32(buf.subarray(0, TNP_HEADER_LEN + payloadLen));

    if (computedChecksum !== expectedChecksum) {
      throw new ProtocolError(
        `CRC32 checksum mismatch: expected ${expectedChecksum}, computed ${computedChecksum}`
      );
    }

    const payload = Buffer.from(buf.subarray(TNP_HEADER_LEN, TNP_HEADER_LEN + payloadLen));
    return {
      packet: new TnpPacket(msgType, streamId, payload, version),
      consumed: totalLen,
    };
  }
}

export function encodeHandshakeRequest(clientId: string, capabilityFlags: bigint = 0x01n): Buffer {
  const idBytes = Buffer.from(clientId, "utf-8");
  const buf = Buffer.alloc(2 + 4 + idBytes.length + 8);
  buf.writeUInt16LE(TNP_VERSION, 0);
  buf.writeUInt32LE(idBytes.length, 2);
  idBytes.copy(buf, 6);
  buf.writeBigUInt64LE(capabilityFlags, 6 + idBytes.length);
  return buf;
}

export function decodeHandshakeResponse(payload: Buffer): {
  success: boolean;
  negotiatedVersion: number;
  serverId: string;
  capabilityFlags: bigint;
} {
  if (payload.length < 7) {
    throw new ProtocolError("Truncated HandshakeResponse");
  }
  const success = payload.readUInt8(0) !== 0;
  const negotiatedVersion = payload.readUInt16LE(1);
  const idLen = payload.readUInt32LE(3);
  if (payload.length < 7 + idLen + 8) {
    throw new ProtocolError("Truncated HandshakeResponse serverId");
  }
  const serverId = payload.subarray(7, 7 + idLen).toString("utf-8");
  const capabilityFlags = payload.readBigUInt64LE(7 + idLen);
  return { success, negotiatedVersion, serverId, capabilityFlags };
}

export function encodeQueryRequest(queryStr: string, format: QueryFormat = QueryFormat.Sql, maxRows: number = 1000): Buffer {
  const qBytes = Buffer.from(queryStr, "utf-8");
  const buf = Buffer.alloc(1 + 4 + 4 + qBytes.length);
  buf.writeUInt8(format, 0);
  buf.writeUInt32LE(maxRows, 1);
  buf.writeUInt32LE(qBytes.length, 5);
  qBytes.copy(buf, 9);
  return buf;
}

export function encodeSubscribeRequest(
  queryStr: string,
  format: QueryFormat = QueryFormat.Sql,
  fromSequence?: bigint,
  fromNow: boolean = false
): Buffer {
  const qBytes = Buffer.from(queryStr, "utf-8");
  const buf = Buffer.alloc(1 + 1 + 8 + 4 + qBytes.length);
  buf.writeUInt8(format, 0);
  buf.writeUInt8(fromNow ? 1 : 0, 1);
  buf.writeBigUInt64LE(fromSequence ?? 0n, 2);
  buf.writeUInt32LE(qBytes.length, 10);
  qBytes.copy(buf, 14);
  return buf;
}

export function decodeSubscribeResponse(payload: Buffer): {
  subscriptionId: bigint;
  success: boolean;
  snapshotStart: bigint;
  snapshotEnd: bigint;
  message: string;
} {
  if (payload.length < 29) {
    throw new ProtocolError("Truncated SubscribeResponse");
  }
  const subscriptionId = payload.readBigUInt64LE(0);
  const success = payload.readUInt8(8) !== 0;
  const snapshotStart = payload.readBigUInt64LE(9);
  const snapshotEnd = payload.readBigUInt64LE(17);
  const msgLen = payload.readUInt32LE(25);
  if (payload.length < 29 + msgLen) {
    throw new ProtocolError("Truncated SubscribeResponse message");
  }
  const message = payload.subarray(29, 29 + msgLen).toString("utf-8");
  return { subscriptionId, success, snapshotStart, snapshotEnd, message };
}

export function decodeLiveEvent(payload: Buffer): LiveEvent {
  if (payload.length < 61) {
    throw new ProtocolError("Truncated LiveEventRecord");
  }
  const subscriptionId = payload.readBigUInt64LE(0);
  const sequence = payload.readBigUInt64LE(8);
  const isLive = payload.readUInt8(16) !== 0;
  const shard = payload.readUInt32LE(17);
  const slot = payload.readUInt32LE(21);
  const generation = payload.readUInt32LE(25);
  const schema = payload.readUInt32LE(29);
  const validClock = payload.readUInt32LE(33);
  const validTime = payload.readBigUInt64LE(37);
  const knownClock = payload.readUInt32LE(45);
  const knownTime = payload.readBigUInt64LE(49);
  const hexLen = payload.readUInt32LE(57);

  if (payload.length < 61 + hexLen) {
    throw new ProtocolError("Truncated LiveEventRecord payload_hex");
  }
  const payloadHex = payload.subarray(61, 61 + hexLen).toString("utf-8");
  const payloadBytes = payloadHex.length > 0 ? Buffer.from(payloadHex, "hex") : new Uint8Array(0);

  return {
    subscriptionId,
    sequence,
    isLive,
    shard,
    slot,
    generation,
    entityId: `#${shard}:${slot}:${generation}`,
    schema,
    validClock,
    validTime,
    knownClock,
    knownTime,
    payloadHex,
    payloadBytes,
  };
}

export function encodeUnsubscribeRequest(subscriptionId: bigint): Buffer {
  const buf = Buffer.alloc(8);
  buf.writeBigUInt64LE(subscriptionId, 0);
  return buf;
}

export function decodeUnsubscribeResponse(payload: Buffer): {
  subscriptionId: bigint;
  success: boolean;
} {
  if (payload.length < 9) {
    throw new ProtocolError("Truncated UnsubscribeResponse");
  }
  const subscriptionId = payload.readBigUInt64LE(0);
  const success = payload.readUInt8(8) !== 0;
  return { subscriptionId, success };
}
