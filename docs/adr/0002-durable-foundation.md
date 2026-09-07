# ADR 0002: Durable source logs before adaptive storage

Status: accepted implementation contract for M3/M4. Later format changes require
new version identifiers and compatibility fixtures, not reinterpretation.

## Record and identity

A database has an OS-random 128-bit DatabaseId persisted before admitting data.
EventId remains source/epoch/sequence, qualified by its database. A reopened log
resumes its durable sequence; it never restarts at zero under an existing epoch.
SchemaId and FieldId are stable numeric identities, never repurposed. BranchId
identifies a timeline; the first durable source-log implementation is single
branch and does not advertise branching until its manifests are implemented.

The persisted record contains EventId, EntityId, all EventTimes, SchemaId,
owned payload bytes, and optional causal EventId references. Typed mutation
encoding is independent of WAL/segment framing. No Rust struct layout or JSON
encoding is used as an implicit persistent format.

Valid/observed clocks remain independent. Known time is nondecreasing within a
source epoch, with sequence breaking ties. The caller's known time is not an OS
wall clock or a claim about knowledge from other sources.

## Durability and failure

Only one writer may open a database directory, enforced by an OS file lock.
Append validates the complete batch before writing one bounded checksummed WAL
frame. A durable receipt is returned only after File::sync_all succeeds.
This is the operating system's synchronization contract, not protection against
broken filesystems, hardware that ignores flushes, or loss of the storage device.

A write or synchronization error poisons the current writer. Its result has an
unknown commit outcome: reopen/recover and inspect before retrying. Never
continue assigning IDs after an ambiguous I/O failure.

Recovery verifies every complete frame and all identity/order invariants.
Complete-frame checksum or semantic corruption is an error, never skipped.
Incomplete trailing records are reported separately. Truncating an incomplete
tail requires explicit recovery permission and reports discarded bytes; it does
not silently discard data during an ordinary open.

Existing logs remain authoritative while immutable TSF export/sealing is
introduced. Do not retire WAL data until a separately verified manifest and
retirement protocol exists. A segment file is written to a unique temporary
file, validated, synchronized and published without overwriting existing
immutable evidence. Unix directory entries are synchronized where required.
Platform publication semantics and limitations must be explicit.

## Resource and format bounds

All lengths, counts, checksums and arithmetic are checked before allocation.
Framing protects its length metadata as well as payload content. Unknown format
versions, flags and incompatible IDs are explicit errors.

Default limits start at 4 MiB per WAL frame, 1 MiB per payload, 65,536 records per
batch and 64 causes per event. These are limits, not preallocation requirements.
The format API allows smaller limits for tests and deployments. Segment readers
also enforce an explicit segment byte and record bound.

Queries and replay operate on a consistent source-log prefix with bounded
results/work. Schema validation, payload encoding and exact replay semantics
are tested separately from persistence. Compression must be lossless and
equivalent to the raw representation before it can be selected.
