# SPDX-License-Identifier: AGPL-3.0-only
"""
Unit and integration tests for the Temnion Python client.
"""

import struct
import unittest
import zlib

from temnion.protocol import (
    MessageType,
    TNP_MAGIC,
    TNP_VERSION,
    TnpPacket,
    decode_handshake_response,
    decode_live_event,
    decode_subscribe_response,
    decode_unsubscribe_response,
    encode_handshake_request,
    encode_query_request,
    encode_subscribe_request,
    encode_unsubscribe_request,
)
from temnion.types import ProtocolError, QueryFormat


class TestProtocolFraming(unittest.TestCase):
    """Verifies binary framing byte-for-byte against TNP specification."""

    def test_packet_encode_decode_roundtrip(self):
        payload = b"Hello Temnion Database Engine!"
        packet = TnpPacket(MessageType.PING, stream_id=42, payload=payload)
        encoded = packet.encode()

        self.assertTrue(encoded.startswith(TNP_MAGIC))
        # Total length: 16 (header) + len(payload) + 4 (crc)
        self.assertEqual(len(encoded), 16 + len(payload) + 4)

        decoded, consumed = TnpPacket.decode(encoded)
        self.assertEqual(consumed, len(encoded))
        self.assertEqual(decoded.version, TNP_VERSION)
        self.assertEqual(decoded.message_type, MessageType.PING)
        self.assertEqual(decoded.stream_id, 42)
        self.assertEqual(decoded.payload, payload)

    def test_checksum_corruption_detection(self):
        packet = TnpPacket(MessageType.PING, stream_id=1, payload=b"SecureData")
        encoded = bytearray(packet.encode())

        # Corrupt one payload byte
        encoded[18] ^= 0xFF

        with self.assertRaises(ProtocolError) as cm:
            TnpPacket.decode(bytes(encoded))
        self.assertIn("Checksum mismatch", str(cm.exception))

    def test_handshake_payload_codec(self):
        client_id = "test-agent-42"
        req_bytes = encode_handshake_request(client_id, capability_flags=0x07)

        # HandshakeResponse simulation
        server_id = "temnion-node-1"
        resp_payload = (
            struct.pack("<BHI", 1, TNP_VERSION, len(server_id.encode("utf-8")))
            + server_id.encode("utf-8")
            + struct.pack("<Q", 0x01)
        )

        success, negotiated_ver, sid, caps = decode_handshake_response(resp_payload)
        self.assertTrue(success)
        self.assertEqual(negotiated_ver, TNP_VERSION)
        self.assertEqual(sid, server_id)
        self.assertEqual(caps, 0x01)

    def test_query_request_codec(self):
        sql = "SELECT * FROM events WHERE sequence > 100"
        payload = encode_query_request(sql, format_=QueryFormat.SQL, max_rows=500)
        self.assertEqual(payload[0], int(QueryFormat.SQL))
        max_rows = struct.unpack_from("<I", payload, 1)[0]
        self.assertEqual(max_rows, 500)
        q_len = struct.unpack_from("<I", payload, 5)[0]
        self.assertEqual(q_len, len(sql.encode("utf-8")))
        self.assertEqual(payload[9:].decode("utf-8"), sql)

    def test_subscribe_request_codec(self):
        query = "SELECT * FROM events"
        payload = encode_subscribe_request(query, format_=QueryFormat.TEMQL, from_sequence=12345, from_now=False)
        self.assertEqual(payload[0], int(QueryFormat.TEMQL))
        self.assertEqual(payload[1], 0)  # from_now = False
        seq = struct.unpack_from("<Q", payload, 2)[0]
        self.assertEqual(seq, 12345)

    def test_subscribe_response_codec(self):
        resp_payload = (
            struct.pack("<Q B QQ I", 99, 1, 10, 50, len(b"Subscription active"))
            + b"Subscription active"
        )
        sub_id, success, snap_start, snap_end, msg = decode_subscribe_response(resp_payload)
        self.assertEqual(sub_id, 99)
        self.assertTrue(success)
        self.assertEqual(snap_start, 10)
        self.assertEqual(snap_end, 50)
        self.assertEqual(msg, "Subscription active")

    def test_live_event_record_decode(self):
        hex_data = "DEADBEEF42"
        hex_bytes = hex_data.encode("utf-8")
        raw_payload = (
            struct.pack(
                "<QQBIIIIIQIQI",
                1001,       # sub_id
                42,         # seq
                1,          # is_live
                0,          # shard
                1,          # slot
                1,          # generation
                1,          # schema
                1,          # valid_clock
                1000000,    # valid_time
                1,          # known_clock
                2000000,    # known_time
                len(hex_bytes),
            )
            + hex_bytes
        )

        event = decode_live_event(raw_payload)
        self.assertEqual(event.subscription_id, 1001)
        self.assertEqual(event.sequence, 42)
        self.assertTrue(event.is_live)
        self.assertEqual(event.shard, 0)
        self.assertEqual(event.slot, 1)
        self.assertEqual(event.generation, 1)
        self.assertEqual(event.entity_id, "#0:1:1")
        self.assertEqual(event.schema, 1)
        self.assertEqual(event.valid_time, 1000000)
        self.assertEqual(event.known_time, 2000000)
        self.assertEqual(event.payload_hex, "DEADBEEF42")
        self.assertEqual(event.payload_bytes, bytes.fromhex("DEADBEEF42"))

    def test_unsubscribe_codec(self):
        req_bytes = encode_unsubscribe_request(42)
        self.assertEqual(req_bytes, struct.pack("<Q", 42))

        resp_bytes = struct.pack("<QB", 42, 1)
        sub_id, success = decode_unsubscribe_response(resp_bytes)
        self.assertEqual(sub_id, 42)
        self.assertTrue(success)


if __name__ == "__main__":
    unittest.main()
