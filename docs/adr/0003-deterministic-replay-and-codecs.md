# ADR 0003: Deterministic replay, checkpoints, and lossless codecs

Status: accepted implementation contract for M3 and M5. Later format changes require
new version identifiers and compatibility fixtures, not reinterpretation.

## State reconstruction and replay model

Following Temnion Architecture §10, exact historical state is governed by the
discrete state-transition equation:

$$S(t+1) = F(S(t), E(t), \text{Actions}(t), \text{RNG}(t))$$

Rather than persisting full snapshot dumps after every operation, Temnion records:
1. Periodic, atomic, checksummed checkpoints pinned to exact WAL sequences.
2. Contiguous WAL event records from the source log.
3. Deterministic pseudo-random number generator (PRNG) state snapshots (`SplitMix64` step-counted seed and generation).

A reconstruction request at target sequence $N$:
- Identifies the latest durable checkpoint at sequence $K \le N$.
- Loads and checksum-verifies the checkpoint state payload.
- Restores the exact RNG generator state recorded at sequence $K$.
- Sequentially replays contiguous events from $K+1$ up to $N$ through a deterministic `StateReducer`.

Equivalence contract:
- Bit-for-bit identity is guaranteed: fresh reconstruction from sequence 0 must yield identical entity state, timestamps, and RNG snapshots as reconstruction resumed from a checkpoint.

## Checkpoint storage and integrity

Checkpoints are serialized using an explicit binary format (`TNCP` magic, version 1, 104-byte header, CRC32C checksum):
- Includes database ID, source ID, source epoch, sequence, valid timestamp, known timestamp, RNG seed, RNG step count, and payload length.
- Checkpoint files are written to unique temporary files (`.tmp-<id>.tcp`), synchronized via OS `sync_all`, and published atomically via atomic rename.
- Tampered or corrupted checkpoints are rejected upon deserialization without corrupting in-memory state or silently dropping records.

## Lossless codec framework and dynamic scoring

Following Temnion Architecture §12, all compressed representations are strictly
lossless and bit-for-bit reversible. No lossy compression is permitted.

Implemented codec primitives (`TNCX` magic, version 1, CRC32C framing):
1. `RawCodec` (id 0): Passthrough baseline with CRC32C integrity validation.
2. `RleCodec` (id 1): Run-Length Encoding with run and literal tags, optimal for repeated runs.
3. `BitPackCodec` (id 2): Bit-packing for small integers up to 64-bit bounds.
4. `DeltaForCodec` (id 3): Frame-of-Reference combined with delta offsets, optimal for monotonic timestamps and clustered sequences.
5. `XorCodec` (id 4): Leading/trailing zero XOR difference compression, optimal for slowly drifting floats and sensor telemetry.

Dynamic scoring and selection:
- Candidate codecs are evaluated using a cost model factoring compressed size, decompression speed ($\alpha$), and compression speed ($\beta$):
  $$\text{Score} = \text{CompressedBytes} + \alpha \cdot \text{DecodeNs} + \beta \cdot \text{EncodeNs}$$
- A candidate codec is selected if and only if it strictly outperforms the `RawCodec` baseline in compressed size and produces lower score.
- Roundtrip decoding is verified lossless prior to candidate acceptance. If any candidate corrupts or mismatches data, it is rejected and raw encoding is retained.
