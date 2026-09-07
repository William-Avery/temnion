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
