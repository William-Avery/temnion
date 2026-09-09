# Temnion Python Client (`temnion`)

Official, zero-dependency Python client library for [Temnion](https://github.com/William-Avery/temnion), the Rust-first standalone exact-history, bi-temporal, and causal database.

---

## Features

- **Zero Third-Party Runtime Dependencies**: Built entirely using Python's standard library (`socket`, `struct`, `zlib`, `dataclasses`).
- **Binary TNP Framing**: High-throughput binary protocol framing with CRC32 verification matching `temniond`.
- **Tri-Dialect Querying**: Native support for standard SQL, TemQL, and Compact Tem (`tn:`).
- **Live CDC Streaming**: Real-time push predicate subscription streaming (`SubscribeRequest` / `LiveEvent`).
- **Context Manager Support**: Clean `with TemnionClient() as client:` lifecycle semantics.

---

## Installation

```bash
pip install temnion
```

Or install locally from source:

```bash
cd clients/python
pip install -e .
```

---

## Quickstart

### 1. Connecting and Querying

```python
from temnion import TemnionClient, QueryFormat

# Connect to running temniond daemon (default port: 9180)
with TemnionClient(host="127.0.0.1", port=9180) as client:
    # Measure network ping latency
    latency = client.ping()
    print(f"Ping latency: {latency:.2f}ms")

    # Inspect server capabilities
    desc = client.describe()
    print(f"Connected to {desc.server_id} running TNP v{desc.version}")
    print(f"Capabilities: {', '.join(desc.capabilities)}")

    # Execute standard SQL query
    result = client.query("SELECT * FROM events WHERE entity = '#0:1:1'", format_=QueryFormat.SQL)
    print(f"Scanned {result.events_scanned} events, matched {len(result)} rows:")
    for row in result:
        print(f"  Entity: {row.entity_id} | Valid: {row.valid_time} | Sequence: {row.sequence}")
```

### 2. Real-Time Predicate Streaming (CDC)

```python
from temnion import TemnionClient, QueryFormat

with TemnionClient() as client:
    # Subscribe to live commits matching a predicate
    sub = client.subscribe(
        query_str="SELECT * FROM events WHERE schema = 1",
        format_=QueryFormat.SQL,
        from_now=True,
    )
    print(f"Live subscription {sub.subscription_id} active...")

    # Stream incoming events
    for event in sub.events(timeout=30.0):
        print(f"CDC Event: entity={event.entity_id} seq={event.sequence} payload_hex={event.payload_hex}")
```

---

## License

AGPL-3.0-only. Commercial licenses available at licensing@temnion.com.
