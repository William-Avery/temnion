# SPDX-License-Identifier: AGPL-3.0-only
"""
High-level client interface for connecting to Temnion databases over TNP.
"""

import json
import socket
import time
from typing import Generator, List, Optional

from .protocol import (
    MessageType,
    TNP_HEADER_LEN,
    TNP_OVERHEAD,
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
from .types import (
    ConnectionError,
    LiveEvent,
    ProtocolError,
    QueryError,
    QueryFormat,
    QueryResult,
    QueryRow,
    ServerDescription,
    SubscriptionError,
)

DEFAULT_HOST = "127.0.0.1"
DEFAULT_PORT = 9180
DEFAULT_DATABASE = "temnion_default"
DEFAULT_USER = "temnion_admin"


class SubscriptionHandle:
    """Handle to an active live event stream."""

    def __init__(self, client: "TemnionClient", subscription_id: int, snapshot_start: int, snapshot_end: int):
        self._client = client
        self.subscription_id = subscription_id
        self.snapshot_start = snapshot_start
        self.snapshot_end = snapshot_end
        self._closed = False

    def __iter__(self) -> Generator[LiveEvent, None, None]:
        """Iterates over incoming live CDC events."""
        return self.events()

    def events(self, timeout: Optional[float] = None) -> Generator[LiveEvent, None, None]:
        """Generator yielding live CDC events as they are committed to storage."""
        while not self._closed:
            try:
                pkt = self._client._recv_packet(timeout=timeout)
                if pkt.message_type == MessageType.LIVE_EVENT:
                    event = decode_live_event(pkt.payload)
                    if event.subscription_id == self.subscription_id:
                        yield event
                elif pkt.message_type == MessageType.ERROR_RESPONSE:
                    err_msg = pkt.payload.decode("utf-8", errors="replace")
                    raise SubscriptionError(f"Subscription error from server: {err_msg}")
            except socket.timeout:
                if timeout is not None:
                    break
                continue
            except (ConnectionError, OSError):
                break

    def unsubscribe(self):
        """Unsubscribes from the live event stream."""
        if self._closed:
            return
        self._closed = True
        req_bytes = encode_unsubscribe_request(self.subscription_id)
        pkt = TnpPacket(MessageType.UNSUBSCRIBE_REQUEST, 1, req_bytes)
        self._client._send_packet(pkt)
        resp_pkt = self._client._recv_packet()
        if resp_pkt.message_type == MessageType.UNSUBSCRIBE_RESPONSE:
            decode_unsubscribe_response(resp_pkt.payload)


class TemnionClient:
    """Temnion database client providing connection management, querying, and live CDC subscriptions."""

    def __init__(
        self,
        host: str = DEFAULT_HOST,
        port: int = DEFAULT_PORT,
        database: str = DEFAULT_DATABASE,
        username: str = DEFAULT_USER,
        auth_token: Optional[str] = None,
        timeout: float = 10.0,
    ):
        self.host = host
        self.port = port
        self.database = database
        self.username = username
        self.auth_token = auth_token
        self.timeout = timeout
        self._sock: Optional[socket.socket] = None
        self._recv_buf = bytearray()
        self.server_id: Optional[str] = None
        self.negotiated_version: Optional[int] = None
        self.capability_flags: int = 0

    def connect(self) -> "TemnionClient":
        """Establishes TCP connection and performs protocol handshake."""
        if self._sock is not None:
            return self

        try:
            self._sock = socket.create_connection((self.host, self.port), timeout=self.timeout)
            self._sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        except OSError as e:
            raise ConnectionError(f"Failed to connect to Temnion at {self.host}:{self.port} - {e}") from e

        # Send HandshakeRequest
        client_id = f"{self.username}:python-sdk"
        req_payload = encode_handshake_request(client_id, capability_flags=0x01)
        req_pkt = TnpPacket(MessageType.HANDSHAKE_REQUEST, 1, req_payload)
        self._send_packet(req_pkt)

        # Receive HandshakeResponse
        resp_pkt = self._recv_packet()
        if resp_pkt.message_type != MessageType.HANDSHAKE_RESPONSE:
            self.close()
            raise ProtocolError(f"Expected HandshakeResponse, got message type {resp_pkt.message_type:#06x}")

        success, negotiated_ver, server_id, cap_flags = decode_handshake_response(resp_pkt.payload)
        if not success:
            self.close()
            raise ConnectionError(f"Server rejected handshake: server_id={server_id}")

        self.server_id = server_id
        self.negotiated_version = negotiated_ver
        self.capability_flags = cap_flags
        return self

    def close(self):
        """Closes the connection socket."""
        if self._sock is not None:
            try:
                self._sock.close()
            except OSError:
                pass
            self._sock = None
        self._recv_buf.clear()

    def __enter__(self) -> "TemnionClient":
        return self.connect()

    def __exit__(self, exc_type, exc_val, exc_tb):
        self.close()

    def ping(self) -> float:
        """Pings the server and returns round-trip latency in milliseconds."""
        payload = b"PING"
        pkt = TnpPacket(MessageType.PING, 1, payload)
        start = time.perf_counter()
        self._send_packet(pkt)
        resp = self._recv_packet()
        latency_ms = (time.perf_counter() - start) * 1000.0
        if resp.message_type != MessageType.PONG:
            raise ProtocolError(f"Expected PONG, got {resp.message_type:#06x}")
        return latency_ms

    def describe(self) -> ServerDescription:
        """Queries the server for capability and version metadata."""
        pkt = TnpPacket(MessageType.DESCRIBE_REQUEST, 1, b"")
        self._send_packet(pkt)
        resp = self._recv_packet()
        if resp.message_type != MessageType.DESCRIBE_RESPONSE:
            raise ProtocolError(f"Expected DescribeResponse, got {resp.message_type:#06x}")

        try:
            data = json.loads(resp.payload.decode("utf-8"))
            return ServerDescription(
                server_id=data.get("server_id", ""),
                version=data.get("version", 1),
                capabilities=data.get("capabilities", []),
            )
        except json.JSONDecodeError as e:
            raise ProtocolError(f"Invalid JSON in DescribeResponse: {e}") from e

    def query(
        self,
        query_str: str,
        format_: QueryFormat = QueryFormat.SQL,
        max_rows: int = 1000,
    ) -> QueryResult:
        """Executes a SQL, TemQL, or Compact Tem query and returns the materialized result set."""
        req_payload = encode_query_request(query_str, format_=format_, max_rows=max_rows)
        pkt = TnpPacket(MessageType.QUERY_REQUEST, 1, req_payload)
        self._send_packet(pkt)

        # 1. QueryResponse
        resp_pkt = self._recv_packet()
        if resp_pkt.message_type == MessageType.ERROR_RESPONSE:
            err_msg = resp_pkt.payload.decode("utf-8", errors="replace")
            raise QueryError(f"Query execution failed: {err_msg}")
        if resp_pkt.message_type != MessageType.QUERY_RESPONSE:
            raise ProtocolError(f"Expected QueryResponse, got {resp_pkt.message_type:#06x}")

        try:
            meta = json.loads(resp_pkt.payload.decode("utf-8"))
            events_scanned = meta.get("scanned", 0)
            truncated = meta.get("truncated", False)
        except json.JSONDecodeError as e:
            raise ProtocolError(f"Invalid QueryResponse header JSON: {e}") from e

        # 2. StreamRecord packets until StreamEnd
        rows: List[QueryRow] = []
        while True:
            item_pkt = self._recv_packet()
            if item_pkt.message_type == MessageType.STREAM_END:
                break
            elif item_pkt.message_type == MessageType.STREAM_RECORD:
                # Format: shard:slot:gen|valid_time|known_time|sequence
                record_str = item_pkt.payload.decode("utf-8")
                parts = record_str.split("|")
                if len(parts) == 4:
                    entity_parts = parts[0].split(":")
                    if len(entity_parts) == 3:
                        shard, slot, gen = int(entity_parts[0]), int(entity_parts[1]), int(entity_parts[2])
                        v_time = int(parts[1])
                        k_time = int(parts[2])
                        seq = int(parts[3])
                        rows.append(QueryRow(shard, slot, gen, v_time, k_time, seq))
            elif item_pkt.message_type == MessageType.ERROR_RESPONSE:
                err_msg = item_pkt.payload.decode("utf-8", errors="replace")
                raise QueryError(f"Query streaming error: {err_msg}")

        return QueryResult(
            rows_count=len(rows),
            events_scanned=events_scanned,
            truncated=truncated,
            rows=rows,
        )

    def subscribe(
        self,
        query_str: str,
        format_: QueryFormat = QueryFormat.SQL,
        from_sequence: Optional[int] = None,
        from_now: bool = False,
    ) -> SubscriptionHandle:
        """Establishes a live CDC subscription streaming real-time event commits."""
        req_payload = encode_subscribe_request(
            query_str,
            format_=format_,
            from_sequence=from_sequence,
            from_now=from_now,
        )
        pkt = TnpPacket(MessageType.SUBSCRIBE_REQUEST, 1, req_payload)
        self._send_packet(pkt)

        resp_pkt = self._recv_packet()
        if resp_pkt.message_type != MessageType.SUBSCRIBE_RESPONSE:
            raise ProtocolError(f"Expected SubscribeResponse, got {resp_pkt.message_type:#06x}")

        sub_id, success, snap_start, snap_end, msg = decode_subscribe_response(resp_pkt.payload)
        if not success:
            raise SubscriptionError(f"Subscription rejected by server: {msg}")

        return SubscriptionHandle(self, sub_id, snap_start, snap_end)

    def _send_packet(self, packet: TnpPacket):
        if self._sock is None:
            raise ConnectionError("Client is not connected")
        data = packet.encode()
        self._sock.sendall(data)

    def _recv_packet(self, timeout: Optional[float] = None) -> TnpPacket:
        if self._sock is None:
            raise ConnectionError("Client is not connected")

        if timeout is not None:
            self._sock.settimeout(timeout)
        else:
            self._sock.settimeout(self.timeout)

        while True:
            # Check if complete packet is in buffer
            if len(self._recv_buf) >= TNP_OVERHEAD:
                # peek payload length
                import struct
                _, _, _, _, payload_len = struct.unpack_from("<4sHHII", self._recv_buf, 0)
                total_len = TNP_HEADER_LEN + payload_len + 4
                if len(self._recv_buf) >= total_len:
                    packet, consumed = TnpPacket.decode(bytes(self._recv_buf))
                    del self._recv_buf[:consumed]
                    return packet

            # Read more bytes from socket
            chunk = self._sock.recv(65536)
            if not chunk:
                raise ConnectionError("Connection closed by server")
            self._recv_buf.extend(chunk)
