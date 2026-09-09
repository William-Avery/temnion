# SPDX-License-Identifier: AGPL-3.0-only
"""
Type definitions, enums, and exceptions for Temnion Python client.
"""

from dataclasses import dataclass, field
from enum import IntEnum
from typing import List, Optional


class QueryFormat(IntEnum):
    """Supported query dialect formats for Temnion."""
    TEMQL = 1
    COMPACT_TEM = 2
    SQL = 3


@dataclass
class QueryRow:
    """Individual row returned by a Temnion query scan."""
    shard: int
    slot: int
    generation: int
    valid_time: int
    known_time: int
    sequence: int

    @property
    def entity_id(self) -> str:
        """Formatted entity identifier #shard:slot:generation."""
        return f"#{self.shard}:{self.slot}:{self.generation}"


@dataclass
class QueryResult:
    """Result set returned by QueryRequest execution."""
    rows_count: int
    events_scanned: int
    truncated: bool
    rows: List[QueryRow] = field(default_factory=list)

    def __iter__(self):
        return iter(self.rows)

    def __len__(self):
        return len(self.rows)

    def __getitem__(self, index):
        return self.rows[index]


@dataclass
class LiveEvent:
    """Real-time Change-Data-Capture (CDC) event received from a live subscription."""
    subscription_id: int
    sequence: int
    is_live: bool
    shard: int
    slot: int
    generation: int
    schema: int
    valid_clock: int
    valid_time: int
    known_clock: int
    known_time: int
    payload_hex: str

    @property
    def entity_id(self) -> str:
        """Formatted entity identifier #shard:slot:generation."""
        return f"#{self.shard}:{self.slot}:{self.generation}"

    @property
    def payload_bytes(self) -> bytes:
        """Decodes the payload hex string into raw binary bytes."""
        return bytes.fromhex(self.payload_hex) if self.payload_hex else b""


@dataclass
class ServerDescription:
    """Server capability and version metadata returned by DescribeRequest."""
    server_id: str
    version: int
    capabilities: List[str] = field(default_factory=list)


class TemnionError(Exception):
    """Base exception for all Temnion client errors."""
    pass


class ConnectionError(TemnionError):
    """Network connection or TCP handshake failure."""
    pass


class ProtocolError(TemnionError):
    """TNP wire protocol framing, magic, or checksum mismatch."""
    pass


class QueryError(TemnionError):
    """Error executing or parsing a query."""
    pass


class SubscriptionError(TemnionError):
    """Subscription request rejected or streaming error."""
    pass
