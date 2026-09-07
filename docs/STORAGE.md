# Durable source storage

The first persistent storage layer is `temnion-storage::Store`: one
OS-locked source/epoch log per database directory. It adds durable batch
admission and independently readable TSF exports without claiming that the
later shard runtime, tiered query engine or lifecycle compactor exists.

## On-disk layout

```text
database\
  .writer.lock
  events.wal
  segments\
    00000000000000000000-00000000000000000009.tsf
```

The WAL's checksummed header binds a database's OS-random 128-bit identity to
its source and epoch. Complete WAL batch frames contain checksummed records.
Reopen validates all complete frames and resumes the source-local sequence;
it does not invent a new identity or reset a durable sequence.

`segments` contains immutable exports named by their inclusive sequence range.
The current exporter writes one segment per WAL batch. It retains the complete
WAL as the authority: exported segments are not a manifest-based replacement
for recovery and do not yet reduce retained WAL space.

## Run it

```text
cargo run -p temnion-cli -- init local-databases\example
cargo run -p temnion-cli -- append local-databases\example 0:1:0 1 1:5 2:10 2a00
cargo run -p temnion-cli -- inspect local-databases\example
cargo run -p temnion-cli -- history local-databases\example 100
cargo run -p temnion-cli -- seal local-databases\example
```

Use the platform's path spelling when invoking these commands. `append` is a
low-level interface: entity is `shard:slot:generation`, times are `clock:tick`,
and the last argument is an even-length hexadecimal payload. Schema ID is
recorded but this low-level command does not register a schema or validate the
opaque payload against one. Higher-level typed mutation APIs must perform that
validation before admission.

`history` prints a single bounded page with at most 64 payload-preview bytes
per row. Its `more` flag means the snapshot has more work; it does not imply
that all history was printed. Rust callers can follow the opaque
`StorageCursor`. Serialized/remote continuation tokens are not implemented.

## Acknowledgments and errors

`Store::append` validates the whole batch, writes its complete frame, then calls
`File::sync_all`. Only success returns a `DurableReceipt`. Ordinary validation
errors leave the previous file prefix and sequence untouched.

An append I/O failure returns `CommitOutcomeUnknown` and poisons the writer.
No subsequent append or history query on that handle is accepted. Reopen and
inspect before retrying: a valid frame may have reached storage even when an
acknowledgment could not be returned. Exactly-once client retry keys and
cross-source transactions are not promised.

The durability boundary is the operating system's synchronization guarantee.
It is not immunity to broken filesystems, disks that ignore flushes, device loss,
or arbitrary applications modifying a file while bypassing the advisory lock.
Unix database creation also synchronizes directory ancestry. Windows uses the
WAL file's flush operation; TSF exports remain regenerable from that WAL.

## Recovery is explicit

Ordinary open validates header checksums, lengths/counts, every complete batch,
contiguous source identity/sequence and known-time ordering. It fails on an
incomplete trailing frame rather than silently truncating the file.

To authorize truncation of an incomplete tail:

```text
cargo run -p temnion-cli -- recover local-databases\example
```

The result reports the exact number of discarded trailing bytes. This operation
does not skip a complete corrupt frame, ignore a checksum mismatch, reinterpret
an unsupported version, or repair malformed sequence/clock relationships.
Keep a copy of suspect files before an administrative recovery operation.

## Bounded reads and immutable publication

Default limits are 4 MiB per WAL frame, 65,536 records per batch, 1 MiB per
payload, 64 causes per event and 64 MiB per TSF segment input. Parsing validates
framing lengths before allocating. This is a bounded per-frame design, not a
fixed total database size or process-RSS quota.

The store retains an index of WAL batch offsets/ranges, not decoded payloads
for all history. Startup currently scans the entire WAL. Query results own
only selected decoded records; scans are source-local and do not yet use
entity/spatial pruning indexes.

Storage query budgets bound result count, bytes read and **decoded** records.
A complete frame must fit the scan/read budget because its checksum and
records must be validated together. A smaller budget returns `BudgetTooSmall`
with required frame bounds. Resuming in a frame charges its decode cost again.
Empty pages can still have a continuation.

TSF export creates a unique temporary file, writes and synchronizes it, reads
and validates it, and publishes using a hard link that cannot overwrite an
existing segment name. A filesystem without the required hard-link support
returns an explicit error. Existing matching exports make sealing idempotent;
different contents under the same immutable name are an error. Unix segment
directory updates are synchronized. The WAL is never retired by this command.

Interrupted/failed export can leave a `.tmp-*` orphan. Such files are never
treated as published evidence or automatically deleted by ordinary open.
Reference-aware lifecycle cleanup belongs to the later maintenance milestone.

## Remaining work

Manifest-based segment activation/WAL retirement, streaming segment reads,
compression, replay/checkpoints, branches, per-shard actors, tiered storage,
backups, authenticated protocols and schema-aware engine integration are
separate capabilities. The persistent source log is not a completed
implementation of those milestones.
