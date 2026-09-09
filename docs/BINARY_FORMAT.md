# Temnion binary persistence formats

Temnion M4 uses explicit little-endian binary codecs. No Rust layout, JSON, or
schema payload interpretation is implicit in persistence.

## WAL header (`TNWL`, version 1)

`WAL_HEADER_LEN = 40`

| Offset | Size | Field |
| --- | ---: | --- |
| 0 | 4 | magic `TNWL` |
| 4 | 2 | version = 1 |
| 6 | 2 | flags = 0 |
| 8 | 16 | `DatabaseId` |
| 24 | 4 | `SourceId` |
| 28 | 8 | `SourceEpoch` |
| 36 | 4 | CRC32 of bytes `0..36` |

## WAL batch frame (`TNWB`, version 1)

`BATCH_HEADER_LEN = 24`

| Offset | Size | Field |
| --- | ---: | --- |
| 0 | 4 | magic `TNWB` |
| 4 | 2 | version = 1 |
| 6 | 2 | flags = 0 |
| 8 | 4 | total frame bytes, including header |
| 12 | 4 | record count |
| 16 | 4 | CRC32 of record body bytes |
| 20 | 4 | CRC32 of bytes `0..20` |

Each record body is explicitly encoded:

1. `EventId` (`source:u32`, `epoch:u64`, `sequence:u64`)
2. `EntityId` (`shard:u32`, `slot:u32`, `generation:u32`)
3. `valid` timestamp (`clock:u32`, `ticks:u64`)
4. `known` timestamp (`clock:u32`, `ticks:u64`)
5. `schema:u32`
6. `record_flags:u16` (`bit0 = observed present`)
7. `cause_count:u16`
8. `payload_len:u32`
9. optional `observed` timestamp when `bit0` is set
10. `cause_count` encoded `EventId`s
11. `payload_len` opaque payload bytes

Unknown versions or nonzero reserved flags are errors. Lengths and counts are
validated before allocation, and complete malformed frames are rejected instead
of being reported as truncation.

## TSF segment (`TNSF`, version 1)

Version 1 TSF is intentionally primitive: one raw embedded `TNWB` batch plus a
fixed checked footer. It does **not** claim compression, indexes, or directory
support yet; those require a new format version.

Segment header:

| Offset | Size | Field |
| --- | ---: | --- |
| 0 | 4 | magic `TNSF` |
| 4 | 2 | version = 1 |
| 6 | 2 | flags = 0 |
| 8 | 16 | `DatabaseId` |
| 24 | 4 | `SourceId` |
| 28 | 8 | `SourceEpoch` |
| 36 | 4 | embedded batch byte length |
| 40 | 4 | record count |
| 44 | 4 | CRC32 of embedded batch bytes |
| 48 | 4 | CRC32 of bytes `0..48` |

Footer (`TNFT`, 16 bytes, immediately after the embedded batch):

| Offset | Size | Field |
| --- | ---: | --- |
| 0 | 4 | magic `TNFT` |
| 4 | 2 | version = 1 |
| 6 | 2 | flags = 0 |
| 8 | 4 | directory length = 0 in v1 |
| 12 | 4 | CRC32 of bytes `0..12` |

Readers require exact one-object decoding: trailing bytes, count mismatches,
checksum corruption, unsupported versions, and record/header identity mismatch
are explicit errors. Incomplete is reserved for genuinely truncated input.

## Temnion Network Protocol (TNP v1) Framing

TNP packets use strict little-endian framing with CRC32C validation over payload bytes.

Header (`TNPP`, 16 bytes):

| Offset | Size | Field |
| --- | ---: | --- |
| 0 | 4 | magic `TNPP` (`0x544E5050`) |
| 4 | 2 | version = 1 |
| 6 | 2 | `message_type: u16` |
| 8 | 8 | `stream_id: u64` |
| 16 | 4 | `payload_len: u32` |
| 20 | 4 | CRC32 of payload bytes (0 if payload is empty) |

### Message Type Codes

| Code | Hex | Message | Description |
| --- | --- | --- | --- |
| 1 | `0x0001` | `HandshakeRequest` | Client protocol version and capability flags |
| 2 | `0x0002` | `HandshakeResponse` | Server version, server ID, and negotiated capabilities |
| 3 | `0x0003` | `QueryRequest` | One-off query (SQL, TemQL, or Compact Tem) |
| 4 | `0x0004` | `QueryResponse` | Query execution header and column metadata |
| 5 | `0x0005` | `StreamRecord` | Streaming query result record |
| 6 | `0x0006` | `StreamEnd` | Terminal marker for query result streams |
| 7 | `0x0007` | `Ping` | Heartbeat keep-alive probe |
| 8 | `0x0008` | `Pong` | Heartbeat keep-alive response |
| 9 | `0x0009` | `Error` | Typed error with message payload |
| 10 | `0x000A` | `DescribeRequest` | Server capability introspection probe |
| 11 | `0x000B` | `DescribeResponse` | JSON server descriptor payload |
| 12 | `0x000C` | `SubscribeRequest` | Real-time predicate subscription registration |
| 13 | `0x000D` | `SubscribeResponse` | Subscription acknowledgement with snapshot boundaries |
| 14 | `0x000E` | `LiveEvent` | Streaming committed or historical event record |
| 15 | `0x000F` | `UnsubscribeRequest` | Cancellation request for active subscription |
| 16 | `0x0010` | `UnsubscribeResponse` | Cancellation acknowledgement |

### Live Subscription Message Payloads

#### `SubscribeRequest` (`0x000C`)
- `format: u8` (`0` = TemQL, `1` = CompactTem, `2` = SQL)
- `has_from_seq: u8` (boolean flag)
- `from_seq: u64` (if `has_from_seq == 1`)
- `from_now: u8` (boolean flag; if 1, ignores historical events)
- `query_len: u32`
- `query_bytes: [u8; query_len]`

#### `SubscribeResponse` (`0x000D`)
- `subscription_id: u64`
- `success: u8` (1 = success, 0 = failed)
- `snapshot_start: u64`
- `snapshot_end: u64`
- `msg_len: u32`
- `msg_bytes: [u8; msg_len]`

#### `LiveEvent` (`0x000E`)
- `subscription_id: u64`
- `sequence: u64`
- `is_live: u8` (`0` = historical snapshot catchup, `1` = newly committed live event)
- `entity_shard: u32`
- `entity_slot: u32`
- `entity_generation: u32`
- `schema: u32`
- `valid_clock: u32`
- `valid_time: u64`
- `known_clock: u32`
- `known_time: u64`
- `payload_hex_len: u32`
- `payload_hex_bytes: [u8; payload_hex_len]`

#### `UnsubscribeRequest` (`0x000F`)
- `subscription_id: u64`

#### `UnsubscribeResponse` (`0x0010`)
- `subscription_id: u64`
- `success: u8` (1 = unregistered successfully, 0 = not found)

