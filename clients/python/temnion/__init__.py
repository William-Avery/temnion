# SPDX-License-Identifier: AGPL-3.0-only
"""
Temnion Python Client Library.

A pure Python, high-performance client for interacting with the Temnion
bi-temporal, causal database engine and daemon (temniond).
"""

from .client import TemnionClient, SubscriptionHandle
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
    TemnionError,
)

__version__ = "0.1.0"

__all__ = [
    "TemnionClient",
    "SubscriptionHandle",
    "QueryFormat",
    "QueryResult",
    "QueryRow",
    "LiveEvent",
    "ServerDescription",
    "TemnionError",
    "ConnectionError",
    "ProtocolError",
    "QueryError",
    "SubscriptionError",
]
