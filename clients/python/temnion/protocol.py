# SPDX-License-Identifier: AGPL-3.0-only
"""
Temnion Network Protocol (TNP) packet framing, binary serialization, and CRC32 verification.
"""

import struct
import zlib
from enum import IntEnum
from typing import Optional, Tuple

from .types import LiveEvent, ProtocolError, QueryFormat

TNP_MAGIC = b"TNPP"
TNP_VERSION = 1
TNP_HEADER_LEN = 16
TNP_OVERHEAD = 20
MAX_PACKET_LEN = 16 * 1024 * 1024


class MessageType(IntEnum):
    HANDSHAKE_REQUEST = 0x0001
    HANDSHAKE_RESPONSE = 0x0002
    DESCRIBE_REQUEST = 0x0003
    DESCRIBE_RESPONSE = 0x0004
    QUERY_REQUEST = 0x0005
    QUERY_RESPONSE = 0x0006
    STREAM_RECORD = 0x0007
    STREAM_END = 0x0008
    ERROR_RESPONSE = 0x0009
    PING = 0x000A
    PONG = 0x000B
    SUBSCRIBE_REQUEST = 0x000C
    SUBSCRIBE_RESPONSE = 0x000D
    LIVE_EVENT = 0x000E
    UNSUBSCRIBE_REQUEST = 0x000F
    UNSUBSCRIBE_RESPONSE = 0x0010


class TnpPacket:
    """A framed, checksum-verified TNP packet."""

    def __init__(
        self,
        message_type: int,
        stream_id: int = 1,
        payload: bytes = b"",
        version: int = TNP_VERSION,
    ):
        self.version = version
        self.message_type = message_type
        self.stream_id = stream_id
        self.payload = payload

    def encode(self) -> bytes:
        """Encodes the packet into framed binary bytes with CRC32 checksum."""
        if len(self.payload) > MAX_PACKET_LEN:
            raise ProtocolError(f"Payload size {len(self.payload)} exceeds maximum {MAX_PACKET_LEN}")

        header = struct.pack(
            "<4sHHII",
            TNP_MAGIC,
            self.version,
            int(self.message_type),
            self.stream_id,
            len(self.payload),
        )
        body = header + self.payload
        checksum = zlib.crc32(body) & 0xFFFFFFFF
        return body + struct.pack("<I", checksum)

    @classmethod
    def decode(cls, data: bytes) -> Tuple["TnpPacket", int]:
        """Decodes and validates a TNP packet from binary data. Returns (packet, consumed_bytes)."""
        if len(data) < TNP_OVERHEAD:
            raise ProtocolError("Incomplete packet header")

        magic, version, msg_raw, stream_id, payload_len = struct.unpack_from("<4sHHII", data, 0)

        if magic != TNP_MAGIC:
            raise ProtocolError(f"Invalid magic: expected {TNP_MAGIC!r}, got {magic!r}")

        if version != TNP_VERSION:
            raise ProtocolError(f"Unsupported TNP version: expected {TNP_VERSION}, got {version}")

        if payload_len > MAX_PACKET_LEN:
            raise ProtocolError(f"Payload length {payload_len} exceeds limit")

        total_len = TNP_HEADER_LEN + payload_len + 4
        if len(data) < total_len:
            raise ProtocolError("Incomplete packet payload")

        payload = data[TNP_HEADER_LEN:TNP_HEADER_LEN + payload_len]
        (expected_checksum,) = struct.unpack_from("<I", data, TNP_HEADER_LEN + payload_len)

        computed_checksum = zlib.crc32(data[:TNP_HEADER_LEN + payload_len]) & 0xFFFFFFFF
        if computed_checksum != expected_checksum:
            raise ProtocolError(
                f"Checksum mismatch: computed {computed_checksum:#010x}, expected {expected_checksum:#010x}"
            )

        return cls(msg_raw, stream_id, payload, version), total_len


def encode_handshake_request(client_id: str, capability_flags: int = 0x01) -> bytes:
    id_bytes = client_id.encode("utf-8")
    return struct.pack("<HI", TNP_VERSION, len(id_bytes)) + id_bytes + struct.pack("<Q", capability_flags)


def decode_handshake_response(payload: bytes) -> Tuple[bool, int, str, int]:
    if len(payload) < 7:
        raise ProtocolError("Truncated HandshakeResponse")
    success_byte, negotiated_ver, id_len = struct.unpack_from("<BHI", payload, 0)
    success = (success_byte != 0)
    if len(payload) < 7 + id_len + 8:
        raise ProtocolError("Truncated HandshakeResponse server_id")
    server_id = payload[7:7 + id_len].decode("utf-8")
    (capability_flags,) = struct.unpack_from("<Q", payload, 7 + id_len)
    return success, negotiated_ver, server_id, capability_flags


def encode_query_request(query_str: str, format_: QueryFormat = QueryFormat.SQL, max_rows: int = 1000) -> bytes:
    q_bytes = query_str.encode("utf-8")
    return struct.pack("<BII", int(format_), max_rows, len(q_bytes)) + q_bytes


def encode_subscribe_request(
    query_str: str,
    format_: QueryFormat = QueryFormat.SQL,
    from_sequence: Optional[int] = None,
    from_now: bool = False,
) -> bytes:
    q_bytes = query_str.encode("utf-8")
    from_seq_val = from_sequence if from_sequence is not None else 0
    from_now_byte = 1 if from_now else 0
    return struct.pack("<BBQI", int(format_), from_now_byte, from_seq_val, len(q_bytes)) + q_bytes


def decode_subscribe_response(payload: bytes) -> Tuple[int, bool, int, int, str]:
    if len(payload) < 29:
        raise ProtocolError("Truncated SubscribeResponse")
    sub_id, success_byte, snap_start, snap_end, msg_len = struct.unpack_from("<Q B QQ I", payload, 0)
    success = (success_byte != 0)
    if len(payload) < 29 + msg_len:
        raise ProtocolError("Truncated SubscribeResponse message")
    message = payload[29:29 + msg_len].decode("utf-8")
    return sub_id, success, snap_start, snap_end, message


def decode_live_event(payload: bytes) -> LiveEvent:
    if len(payload) < 61:
        raise ProtocolError("Truncated LiveEventRecord")
    (
        sub_id,
        seq,
        is_live_b,
        shard,
        slot,
        gen,
        schema,
        v_clock,
        v_time,
        k_clock,
        k_time,
        hex_len,
    ) = struct.unpack_from("<QQBIIIIIQIQI", payload, 0)

    is_live = (is_live_b != 0)
    if len(payload) < 61 + hex_len:
        raise ProtocolError("Truncated LiveEventRecord payload_hex")
    payload_hex = payload[61:61 + hex_len].decode("utf-8")

    return LiveEvent(
        subscription_id=sub_id,
        sequence=seq,
        is_live=is_live,
        shard=shard,
        slot=slot,
        generation=gen,
        schema=schema,
        valid_clock=v_clock,
        valid_time=v_time,
        known_clock=k_clock,
        known_time=k_time,
        payload_hex=payload_hex,
    )


def encode_unsubscribe_request(subscription_id: int) -> bytes:
    return struct.pack("<Q", subscription_id)


def decode_unsubscribe_response(payload: bytes) -> Tuple[int, bool]:
    if len(payload) < 9:
        raise ProtocolError("Truncated UnsubscribeResponse")
    sub_id, success_byte = struct.unpack_from("<QB", payload, 0)
    return sub_id, (success_byte != 0)
