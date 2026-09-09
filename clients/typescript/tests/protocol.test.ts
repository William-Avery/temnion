// SPDX-License-Identifier: AGPL-3.0-only
import test from "node:test";
import assert from "node:assert/strict";
import {
  TNP_MAGIC,
  TNP_VERSION,
  MessageType,
  TnpPacket,
  encodeHandshakeRequest,
  decodeHandshakeResponse,
  encodeQueryRequest,
  encodeSubscribeRequest,
  decodeSubscribeResponse,
  decodeLiveEvent,
  encodeUnsubscribeRequest,
  decodeUnsubscribeResponse,
} from "../src/protocol.js";
import { QueryFormat, ProtocolError } from "../src/types.js";
import { crc32 } from "../src/crc32.js";

test("CRC32 calculation matches standard test vectors", () => {
  const data = Buffer.from("123456789", "utf-8");
  // Standard CRC32 of "123456789" is 0xcbf43926
  assert.equal(crc32(data), 0xcbf43926);
});

test("TNP packet encode and decode roundtrip", () => {
  const payload = Buffer.from("Temnion TypeScript Client Test", "utf-8");
  const packet = new TnpPacket(MessageType.Ping, 42, payload);
  const encoded = packet.encode();

  assert.equal(encoded.subarray(0, 4).toString("utf-8"), "TNPP");
  assert.equal(encoded.length, 16 + payload.length + 4);

  const { packet: decoded, consumed } = TnpPacket.decode(encoded);
  assert.equal(consumed, encoded.length);
  assert.equal(decoded.version, TNP_VERSION);
  assert.equal(decoded.messageType, MessageType.Ping);
  assert.equal(decoded.streamId, 42);
  assert.equal(decoded.payload.toString("utf-8"), "Temnion TypeScript Client Test");
});

test("TNP checksum corruption rejection", () => {
  const packet = new TnpPacket(MessageType.Ping, 1, Buffer.from("SecureData"));
  const encoded = packet.encode();

  // Corrupt a byte
  encoded[18] ^= 0xff;

  assert.throws(() => {
    TnpPacket.decode(encoded);
  }, ProtocolError);
});

test("Handshake request and response codecs", () => {
  const req = encodeHandshakeRequest("admin:node", 0x01n);
  assert.ok(req.length > 6);

  // Simulated server handshake response
  const sid = "temnion-server-primary";
  const sidBuf = Buffer.from(sid, "utf-8");
  const respBuf = Buffer.alloc(1 + 2 + 4 + sidBuf.length + 8);
  respBuf.writeUInt8(1, 0); // success
  respBuf.writeUInt16LE(TNP_VERSION, 1);
  respBuf.writeUInt32LE(sidBuf.length, 3);
  sidBuf.copy(respBuf, 7);
  respBuf.writeBigUInt64LE(0x01n, 7 + sidBuf.length);

  const decoded = decodeHandshakeResponse(respBuf);
  assert.equal(decoded.success, true);
  assert.equal(decoded.negotiatedVersion, TNP_VERSION);
  assert.equal(decoded.serverId, sid);
  assert.equal(decoded.capabilityFlags, 0x01n);
});

test("Query request encoding", () => {
  const sql = "SELECT * FROM events WHERE sequence > 10";
  const payload = encodeQueryRequest(sql, QueryFormat.Sql, 250);
  assert.equal(payload.readUInt8(0), QueryFormat.Sql);
  assert.equal(payload.readUInt32LE(1), 250);
  assert.equal(payload.readUInt32LE(5), Buffer.byteLength(sql));
  assert.equal(payload.subarray(9).toString("utf-8"), sql);
});

test("Subscribe request and response encoding/decoding", () => {
  const query = "SELECT * FROM events";
  const req = encodeSubscribeRequest(query, QueryFormat.Temql, 100n, false);
  assert.equal(req.readUInt8(0), QueryFormat.Temql);
  assert.equal(req.readUInt8(1), 0);
  assert.equal(req.readBigUInt64LE(2), 100n);

  const respBuf = Buffer.alloc(29 + 19);
  respBuf.writeBigUInt64LE(777n, 0);
  respBuf.writeUInt8(1, 8);
  respBuf.writeBigUInt64LE(10n, 9);
  respBuf.writeBigUInt64LE(20n, 17);
  respBuf.writeUInt32LE(19, 25);
  respBuf.write("Subscription active", 29, "utf-8");

  const resp = decodeSubscribeResponse(respBuf);
  assert.equal(resp.subscriptionId, 777n);
  assert.equal(resp.success, true);
  assert.equal(resp.snapshotStart, 10n);
  assert.equal(resp.snapshotEnd, 20n);
  assert.equal(resp.message, "Subscription active");
});

test("LiveEventRecord decoding", () => {
  const hex = "CAFE42";
  const hexBuf = Buffer.from(hex, "utf-8");
  const payload = Buffer.alloc(61 + hexBuf.length);

  payload.writeBigUInt64LE(88n, 0); // sub_id
  payload.writeBigUInt64LE(123n, 8); // seq
  payload.writeUInt8(1, 16); // is_live
  payload.writeUInt32LE(0, 17); // shard
  payload.writeUInt32LE(2, 21); // slot
  payload.writeUInt32LE(1, 25); // generation
  payload.writeUInt32LE(1, 29); // schema
  payload.writeUInt32LE(1, 33); // valid_clock
  payload.writeBigUInt64LE(5000n, 37); // valid_time
  payload.writeUInt32LE(1, 45); // known_clock
  payload.writeBigUInt64LE(6000n, 49); // known_time
  payload.writeUInt32LE(hexBuf.length, 57);
  hexBuf.copy(payload, 61);

  const event = decodeLiveEvent(payload);
  assert.equal(event.subscriptionId, 88n);
  assert.equal(event.sequence, 123n);
  assert.equal(event.isLive, true);
  assert.equal(event.entityId, "#0:2:1");
  assert.equal(event.schema, 1);
  assert.equal(event.validTime, 5000n);
  assert.equal(event.knownTime, 6000n);
  assert.equal(event.payloadHex, "CAFE42");
  assert.deepEqual(Array.from(event.payloadBytes), [0xca, 0xfe, 0x42]);
});

test("Unsubscribe request and response codecs", () => {
  const req = encodeUnsubscribeRequest(999n);
  assert.equal(req.readBigUInt64LE(0), 999n);

  const resp = Buffer.alloc(9);
  resp.writeBigUInt64LE(999n, 0);
  resp.writeUInt8(1, 8);

  const decoded = decodeUnsubscribeResponse(resp);
  assert.equal(decoded.subscriptionId, 999n);
  assert.equal(decoded.success, true);
});
