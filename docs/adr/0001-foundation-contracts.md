# ADR 0001: Safe, volatile contracts before persistence

- **Status:** accepted for the M0/M1/M2 in-memory foundation.
- **Scope:** Rust APIs in `temnion-core`, `temnion-state`, and `temnion-events`.
- **Source traceability:** M0–M4, M8, M14–M18, M38; delivery T01/T03, then T04–T06.
- **Decision boundary:** future disk/wire/C contracts are not frozen by this ADR.

## Context

Packed state and exact history need explicit identity, temporal, mutation and
resource contracts before a persistent representation makes mistakes expensive
to change. A useful safe, std-only reference foundation comes first. It must not
advertise volatile admission as durability, source sequence as global causality,
or illustrative updates as a complete reconstruction engine.

## 1. Identity

Current public shapes:

```text
ShardId(u32)
EntityId { shard: ShardId, slot: u32, generation: u32 }
SourceId(u32)
SourceEpoch(u64)
EventId { source: SourceId, epoch: SourceEpoch, sequence: u64 }
ClockId(u32)
```

Entity identity addresses a logical slot, not a dense-array index or OS thread.
Deleting a value and reusing its slot increments the generation. At generation
exhaustion, retire the slot permanently rather than wrap and revive a stale ID.
Stale, missing and wrong-shard accesses fail.

Event IDs are independent of entity IDs and ordered locally by source sequence.
The log assigns a zero-based sequence within its source epoch. Lexicographic
ordering of IDs is not global time or causality. Current supported targets have
at most 64-bit `usize`; bounded Vec capacity limits reachable sequences without
wrapping that namespace.

There is no database registry or persistent allocator yet. Callers must allocate
distinct logical shard identities to unrelated live slabs and distinct
source/epoch identities to unrelated logs. Recreating a slab or restarting a
log with the same identity does not magically preserve old handles. Callers
must not reuse a source epoch when restarting its sequence. Database-qualified
identity, restart allocation and persistence remain pre-format work.

## 2. Time and ordering

```text
Timestamp { clock: ClockId, ticks: u64 }
EventTimes {
    valid: Timestamp,
    observed: Option<Timestamp>,
    known: Timestamp
}
TimeAxis = Valid | Observed | Known
```

- **Valid:** when the represented world event applies.
- **Observed:** optional capture/observation timestamp; absence stays explicit.
- **Known:** when a source says evidence became known to it. In this foundation
  it is caller-supplied, not a database-generated durable commit timestamp.
- **Causal order:** a separate future relation. Source sequence provides only
  local append order; no causal edges or logical-clock service exist yet.

Clock identity carries the caller's origin/unit contract. There is no implicit
wall-clock conversion, cross-clock ordering, registry, correlation or uncertainty
model. Different axes may use different domains.

Within one log, known timestamps use one clock and nondecreasing ticks; equal
known ticks are ordered by sequence. Appends with another known clock or regressing
known time fail. Valid/observed time can arrive out of order, allowing late evidence
without rewriting earlier records.

`TimeRange` is `[start, end)` in exactly one clock. Empty ranges are valid;
reversed ranges fail. A timestamp from another clock does not match. Missing
observations do not match an observed-time filter.

Known-as-of cutoffs are inclusive. A cutoff must match a nonempty log's known
clock; mismatches are errors, not fabricated clock conversions. Historical filters
select evidence; they do not reconstruct historical state or resolve corrections.

## 3. Packed live state

`StateSlab<T>` owns dense values and separate slot, reverse-mapping and free-slot
metadata. Storage is reserved at construction, with explicit allocation/capacity
errors. Zero capacity is valid; capacity beyond the `u32` slot address space is
rejected.

Insert, read, mutable read, remove and iteration use generation-checked entity
handles. Mutation requires exclusive Rust access. Removal uses swap removal,
so surviving IDs remain valid but iteration order is not stable. Capacity counts
allocated/retired slots, not just currently live values.

“Packed” refers to dense values without an `Option<T>` or free-list field in each
value. It does not mean metadata-free, a stable struct-of-arrays ABI, or proven
theoretical-minimum RAM usage. Generic `T` may have padding or heap allocations.

## 4. Volatile event admission

`EventInput<T>` contains entity, times and a typed change payload.
`EventLog<T>` owns a single bounded source/epoch sequence. No whole-state row,
JSON representation, delta algebra or reducer is imposed.

Single append and nonempty `append_batch(Vec<EventInput<T>>)`:

1. Validate remaining capacity and all known-time constraints.
2. Reject the batch without changing the stored prefix if validation fails.
3. Append all accepted records in input order and assign event IDs.
4. Return a `VolatileReceipt` with first/last ID and count.

An empty batch is an error. Full logs reject admission; they do not overwrite a
ring buffer, evict evidence, silently drop events or spill to disk. The batch
argument is consumed; callers needing retry ownership must retain their input
separately. No external event-key deduplication or persistent exactly-once delivery
is promised. Repeating the same payload creates another event.

The event store exposes shared references, not mutable access to admitted
events. With generic Rust payloads, externally shared interior mutability is
still possible; callers requiring immutable evidence must use appropriately
owned immutable payloads. This is not an enforcement boundary against arbitrary
code running in the same process.

An entity need not still exist in live state to have history. The log does not
validate entity membership in a slab. Live-state updates and history append are
separate operations; there is **no atomic transaction across them**.

No file, WAL, TSF, synchronization, recovery or automatic persistence exists.
Dropping the objects or losing the process loses the data. A receipt establishes
only in-memory admission.

## 5. Bounded historical queries

`get(EventId)` checks source/epoch and retrieves an event by local sequence.
`history` accepts:

- optional exact entity identity;
- optional time axis and clock-scoped range;
- optional inclusive known-as-of cutoff;
- positive `max_results` and `max_scanned` budgets;
- an optional opaque in-process continuation.

Results are in source sequence/append order, not sorted by valid time. The first
page pins a prefix of the log. Continuations retain source/epoch, filter,
snapshot length and scan position; later appends are excluded. Wrong-query or
incompatible cursors fail. Caller identity uniqueness is necessary: the cursor
is not a globally authenticated log-instance identifier.

Each page bounds both returned records and examined records. An empty page
**can have a continuation** because the scan budget was exhausted on nonmatching
events. Iterate until `continuation` is absent, not until a page is empty. Budgets
may change between pages; the filter may not.

This is a linear scan, not an index. Page values borrow the log. Cursors are not
serialized, restartable, authorization-scoped, expiring network tokens or a
subscription resume protocol.

Capacity limits bound record count, not arbitrary payload bytes, caller memory,
wall-clock runtime or process RSS. There are no global numerical defaults or
hard byte/token/deadline quotas yet. Examples choose explicit small capacities.
Complete resource and cancellation contracts precede network/API freeze.

## 6. Ownership and Constitution

Start with safe scalar single-owner code and independent reference tests.
Virtual shards will use one foreground writer per owned group, bounded queues
and explicit backpressure; no global mutable hot database lock and no
thread-per-virtual-shard requirement.

The immutable Constitution is a locked architecture boundary, not a shipped
evolution subsystem. Introduce validation, authorization, resource, retention,
promotion and rollback enforcement together with the features they protect.
Do not defer these protections until M38 or allow runtime candidates to edit them.

## 7. Mandatory decisions before persistence/interface freeze

T01/T04–T06 must settle and test:

| Contract | Required decision/evidence |
| --- | --- |
| Identity | Database/source/shard/entity/event/branch/schema namespaces; restart uniqueness; generation/sequence overflow; source-epoch persistence |
| Schemas | Field types/units/dimensions; missing/uncertain time; compatibility and explicit migration; exactness classes |
| Input semantics | Late arrivals, corrections, duplicates, source gaps, retry keys, dedup retention and invalid schema changes |
| Visibility | Durable receipt boundary, group commit, state/event consistency, snapshot visibility and cross-shard consistent cuts |
| Disk formats | Explicit encoding, byte order, version/feature negotiation, lengths/bounds/checksums, WAL/TSF/manifest validation |
| Recovery | Windows/Linux sync rules, torn writes, object/manifest publication, disk-full, crash injection, safe WAL retirement |
| Queries | Shared typed IR; ordering/ties; cancellation; numeric row/byte/event/I/O/deadline/memory/fanout ceilings; typed error and partial-result semantics |
| Cursors/subscriptions | Query/schema/branch/snapshot/auth binding, expiry, snapshot/live handoff, resume/delivery order, disconnects and bounded overflow |
| Lifecycle | Reader/branch/cursor/evidence holds, authorized audited deletion, reference-aware GC, backup/restore and upgrades |
| Security/FFI | Trust boundaries, local access, remote auth/TLS, quotas, C/Arrow ownership, panic containment and candidate isolation |
| Model accounting | Explicit tokenizer identity/policy before exact token-limit claims; isolated versioned model-worker contracts |

No TSF magic bytes, TNP framing, C layout, grammar or stable serialized cursor
is inferred from current Rust types. See [COMPATIBILITY](../COMPATIBILITY.md).

## Consequences and validation

This scope yields a useful reference foundation without false durability or
compatibility promises. It deliberately leaves runtime, schema, persistence,
indexing and reconstruction work incomplete.

The existing Cargo tests cover typed clock/range behavior, invalid/reused handles,
dense removal, capacity and generation exhaustion, reference state operations,
append/query constraints and CLI behavior. Preserve and extend targeted tests
when contracts change; the [benchmark](../BENCHMARKING.md) is not a replacement
for correctness checks. Later durable features require crash/recovery tests,
not just a successful clean restart.
