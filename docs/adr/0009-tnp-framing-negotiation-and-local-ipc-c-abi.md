# ADR 0009: TNP wire protocol framing, negotiation, local IPC transport, Arrow columnar layout, and C ABI

Status: accepted implementation contract for M18 (Protocol & capability layer: TNP) and M19 (Native / C / Arrow / Local IPC).

## Problem and Context

Temnion requires efficient, robust, and extensible interfaces for external consumption:
1. **Network & Wire Protocol (M18):** Clients, drivers, and external tools need a binary protocol (Temnion Network Protocol, or TNP) with checksummed packet framing, explicit version handshake, capability negotiation, and streaming query results.
2. **Local IPC (M19):** High-performance local processes (e.g. sidecars, analytical runtimes, UI processes) need a low-latency duplex stream transport over pipes or sockets without network stack overhead.
3. **Columnar Interchange (M19):** Analytical engines, DataFrame libraries, and ML frameworks require Arrow-compatible columnar memory layouts to avoid per-row deserialization overhead.
4. **Foreign Function Interface / C ABI (M19):** C, C++, and language bindings (Python, Julia, Go) require a stable, panic-safe C ABI that operates without raw pointer hazards or unsafe blocks.

Following Temnion Architecture §7, §29, and §30, the `temnion-protocol` crate implements these capabilities while strictly upholding `#![forbid(unsafe_code)]`.

## Invariants and Guarantees

1. **Deterministic Checksummed Framing:** Every TNP packet begins with magic bytes `b"TNPP"`, an explicit protocol version, message type identifier, stream identifier, and payload length, followed by payload bytes and a CRC32C trailing checksum over header and payload.
2. **Bounds and Denial-of-Service Defense:** Packets enforce a strict maximum length ceiling (`MAX_PACKET_LEN = 16 MB`). Truncated, oversized, or corrupted frames are rejected before allocation or processing.
3. **Explicit Version Negotiation & Capabilities:** Client connections initiate with a `HandshakeRequest`. The server verifies protocol compatibility and responds with a `HandshakeResponse` carrying bitflags for supported capabilities (`query`, `describe`, `stream`). Unsupported versions are cleanly rejected.
4. **Framed Streaming Protocol:** Query execution sends a `QueryResponse` header summary, followed by a sequence of `StreamRecord` frames, and terminates with a `StreamEnd` packet containing the final record count.
5. **Arrow-Compatible Columnar Batching:** `ColumnarBatch` decomposes row-oriented query results into contiguous columnar arrays (`entity_shards`, `entity_slots`, `entity_generations`, `valid_timestamps`, `known_timestamps`, `sequences`, `schema_ids`) with bidirectional conversion (`from_query_rows`, `to_query_rows`).
6. **Panic-Safe Handle-Based C ABI:** The C interface (`temnion_c_*`) uses atomic integer handles backed by thread-safe `HandleRegistry` instances rather than exposed raw pointers. All entry points execute within `std::panic::catch_unwind` boundaries and return standardized integer status codes (`TEMNION_SUCCESS`, `TEMNION_ERR_*`).
7. **Strict Safe Code Mandate:** All protocol framing, IPC stream handling, columnar transformation, and C ABI boundaries enforce `#![forbid(unsafe_code)]`.

## Architecture and Data Structures

### TNP Packet Framing (`TnpPacket` - M18)

- **Packet Structure (20 bytes overhead):**
  - `magic` (4B): `b"TNPP"`
  - `version` (2B, little-endian): `1`
  - `message_type` (2B, little-endian): e.g. `HandshakeRequest` (`0x0001`), `HandshakeResponse` (`0x0002`), `QueryRequest` (`0x0005`), `QueryResponse` (`0x0006`), `StreamRecord` (`0x0007`), `StreamEnd` (`0x0008`), `Ping` (`0x000A`), `Pong` (`0x000B`)
  - `stream_id` (4B, little-endian): multiplexing identifier
  - `payload_len` (4B, little-endian): length in bytes (<= 16 MB)
  - `payload` (`payload_len` bytes)
  - `crc32c` (4B, little-endian): CRC32C checksum of header and payload

### Handshake & Query Payloads (M18)

- `HandshakeRequest`:
  - `client_version: u16`
  - `client_id: String`
  - `capability_flags: u64`
- `HandshakeResponse`:
  - `success: bool`
  - `negotiated_version: u16`
  - `server_id: String`
  - `capability_flags: u64`
- `QueryRequest`:
  - `format: QueryFormat` (`Temql = 1`, `CompactTem = 2`)
  - `query_str: String`
  - `max_rows: u32`

### Local IPC Transport & Server Dispatcher (`TnpChannel`, `TnpServer` - M19)

- `TnpChannel<R: Read, W: Write>`:
  - Duplex packet channel over any `Read + Write` byte streams (anonymous pipes, named pipes, Unix domain sockets, or in-memory buffers).
  - Handles framing, streaming chunk assembly, and buffer drain.
- `TnpServer`:
  - Accepts connections, negotiates handshakes, handles `Ping`/`Pong` liveness checks, and responds to `DescribeRequest` with JSON capabilities.
  - Dispatches `QueryRequest` to `QueryExecutor` against local `Store`, streaming matching records as `StreamRecord` frames and concluding with `StreamEnd`.

### Columnar Layout (`ColumnarBatch` - M19)

- In-memory contiguous vector layout matching Apache Arrow memory specifications:
  - `entity_shards: Vec<u32>`
  - `entity_slots: Vec<u32>`
  - `entity_generations: Vec<u32>`
  - `valid_timestamps: Vec<u64>`
  - `known_timestamps: Vec<u64>`
  - `sequences: Vec<u64>`
  - `schema_ids: Vec<u32>`
- Zero-copy compatible with columnar analytics engines.

### Safe C ABI (`temnion_c_*` - M19)

- `HandleRegistry<T>`: Internal atomic handle allocator and mutex map (`HashMap<u64, Arc<Mutex<T>>>`).
- Exported entry points:
  - `temnion_c_store_open(path: &str, out_handle: &mut u64) -> i32`
  - `temnion_c_store_close(handle: u64) -> i32`
  - `temnion_c_query_execute(store_handle: u64, query_str: &str, out_result_handle: &mut u64) -> i32`
  - `temnion_c_result_row_count(result_handle: u64) -> usize`
  - `temnion_c_result_free(result_handle: u64) -> i32`
- Return status codes:
  - `TEMNION_SUCCESS = 0`
  - `TEMNION_ERR_INVALID_ARGUMENT = -1`
  - `TEMNION_ERR_NOT_FOUND = -2`
  - `TEMNION_ERR_EXECUTION_FAILED = -3`
  - `TEMNION_ERR_PANIC = -4`

## Verification and Testing

1. **Frame Encoding & Decoding:** Roundtrip validation of packet framing, stream multiplexing IDs, and variable payloads.
2. **Corruption & Edge Case Rejection:** Explicit test coverage verifying that corrupted magic, truncated byte streams, altered payloads, and unsupported versions return corresponding `TnpError` variants.
3. **Payload Codecs:** Roundtrip tests for `HandshakeRequest`, `HandshakeResponse`, and `QueryRequest` (both TemQL and Compact Tem).
4. **Local IPC & Server Dispatch:** End-to-end multi-threaded test with an in-memory duplex pipe exercising Handshake, Ping/Pong, Describe, and Query streaming over real storage.
5. **Columnar Batch Roundtrip:** Exact conversion roundtrip between `QueryRow` records and `ColumnarBatch`.
6. **Safe C ABI Lifecycle:** End-to-end database open, query execution, result inspection, and cleanup via `temnion_c_*` APIs, verifying error code propagation for missing handles and invalid syntax.
