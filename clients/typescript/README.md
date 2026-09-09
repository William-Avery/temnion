# Temnion TypeScript / Node.js Client (`@temnion/client`)

Official TypeScript and Node.js client library for [Temnion](https://github.com/William-Avery/temnion), the Rust-first standalone exact-history, bi-temporal, and causal database.

---

## Features

- **Pure TypeScript & Zero Dependencies**: Built directly over Node's `net.Socket` and built-in binary buffer APIs.
- **Binary TNP Framing**: High-throughput binary serialization with standard IEEE 802.3 CRC32 verification.
- **Tri-Dialect Querying**: Supports standard SQL, TemQL, and Compact Tem (`tn:`).
- **Live CDC Streaming**: Real-time push predicate subscription streaming with AsyncIterable (`for await (const event of sub)`) and EventEmitter patterns.
- **Type-Safe**: Full TypeScript definitions for query results, live CDC event records, and schema projections.

---

## Installation

```bash
npm install @temnion/client
```

---

## Quickstart

### 1. Connecting and Querying

```typescript
import { TemnionClient, QueryFormat } from "@temnion/client";

const client = new TemnionClient({
  host: "127.0.0.1",
  port: 9180,
});

await client.connect();

// Measure ping latency
const latency = await client.ping();
console.log(`Ping latency: ${latency.toFixed(2)}ms`);

// Inspect server capabilities
const desc = await client.describe();
console.log(`Connected to ${desc.serverId} (TNP v${desc.version})`);

// Execute SQL query
const result = await client.query("SELECT * FROM events WHERE entity = '#0:1:1'", {
  format: QueryFormat.Sql,
});

console.log(`Scanned ${result.eventsScanned} events, matched ${result.rows.length} rows:`);
for (const row of result.rows) {
  console.log(`  Entity: ${row.entityId} | Valid: ${row.validTime} | Seq: ${row.sequence}`);
}

await client.close();
```

### 2. Live CDC Subscription Streaming

```typescript
import { TemnionClient, QueryFormat } from "@temnion/client";

const client = new TemnionClient();
await client.connect();

// Subscribe to real-time committed events matching a predicate
const sub = await client.subscribe("SELECT * FROM events WHERE schema = 1", {
  format: QueryFormat.Sql,
  fromNow: true,
});

console.log(`Subscription active with ID: ${sub.subscriptionId}`);

// Consume using modern async iterator
for await (const event of sub) {
  console.log(`Live Event received: entity=${event.entityId}, seq=${event.sequence}, hex=${event.payloadHex}`);
}
```

---

## License

AGPL-3.0-only. Commercial licenses available at licensing@temnion.com.
