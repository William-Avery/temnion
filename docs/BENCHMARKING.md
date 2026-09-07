# Benchmarking Temnion

## Current scope: baseline, not a database result

The default `temnion-bench` binary is the in-memory baseline runner. It does not
write files or synchronize a WAL. A separate, explicitly selected `durable`
binary measures synchronized WAL batches and reopen recovery. Neither runner
establishes Gates A/B/C or a production database performance result.

From the repository root with the pinned Rust 1.85.0 toolchain:

```text
cargo run --release -p temnion-bench -- --entities 4096 --events 100000 --iterations 100000
```

The default sizes match that invocation. Options must be positive integers, with
ceilings of 10,000,000 entities and 100,000,000 each for events/iterations. These
are input bounds, not assurance that the workload fits a particular machine.
Large sizes retain multiple representations and may exhaust memory. Invalid,
missing, repeated or unknown options return a nonzero status.

### Implemented workloads

| Workload/output label | Operation and reference | Important limits |
| --- | --- | --- |
| A / `A-current` | Seeded current-value lookups in `StateSlab`, direct `Vec`, and `HashMap` | Same requested values/checksums; direct Vec omits generation and shard validation and is a **lower bound**, not a complete state store |
| B / `B-append` | Validated `EventLog` appends and a pre-reserved raw `Vec` append stream | Vec omits ID assignment and append validation and stores a different envelope; **unvalidated lower bound**, not equivalent database ingestion |
| D / `D-history-scanned` | Entity history through bounded pages versus a linear Vec reference | Both scan the full snapshot; reported operation count is **events scanned**, not matching results or number of history queries |

The generated index sequence uses seed 42. Payloads and entity selection are
synthetic; append valid/known times are monotonic. A compares result checksums;
D compares result count/checksum to the reference. Cargo correctness tests cover
additional invalid, late-input and pagination cases. B's baseline is not an
independent transactional correctness oracle.

The current runner prints metadata comments and CSV:

```text
workload,implementation,operations,elapsed_ns,ns_per_op,checksum
```

Comments include runner version, OS, architecture, package version, configured
sizes and seed. Timing is one aggregate elapsed interval per row; `ns_per_op` is
elapsed time divided by that row's operation count. It is **not p50/p95/p99/p99.9**,
a confidence interval, isolated device latency, or durable throughput.

Setup/preallocation is outside relevant timed loops; history page allocations
remain part of the history path. The process retains state/log/reference
representations together. The harness does not measure total RSS, allocation/
setup cost, storage metadata, cache misses, cold-cache behavior or transport
overhead. Fixed run order and ordinary OS scheduling can bias timing.

## Volatile, buffered and durable are different

Always label the measured acknowledgment boundary:

| Mode | Meaning | Foundation support |
| --- | --- | --- |
| Volatile in-memory admission | Accepted into the process-owned log; process/object loss loses data | Implemented |
| Buffered persistent I/O | Accepted/enqueued or written without the required stable-storage synchronization | Not implemented |
| Durable acknowledgment | WAL `File::sync_all` completed before acknowledging; reopen validates complete frames | Implemented by the durable source log |
| End-to-end durable ingestion | Producer/queue/validation/write/sync/receipt latency and throughput, with all relevant boundaries included | Not implemented |

The source's “buffered ingestion” target must not be read as a durable target.
Even future successful `write` calls are not sufficient to claim durable commits.
Report batch/group-commit size, synchronization policy, queue depth, filesystems,
storage configuration and concurrency. Compare engines at equivalent recovery,
ordering, consistency, exactness and acknowledgment semantics.

### On-disk batch baseline

The `durable` binary requires a new output directory and leaves its files for
inspection. Its parent directory must already exist. It never overwrites an
existing benchmark directory.

```text
cargo run --release -p temnion-bench --bin durable -- --directory local-databases\durable-run-1 --events 4096 --batch-size 256
```

It appends synthetic eight-byte values in source order, synchronizing once per
batch, then reopens the store and verifies the recovered count/final value.
It compares against a separately written file using the same headers, framing,
payloads and per-batch synchronization. Streaming byte comparison verifies the
two WAL files agree exactly.

The framed-file comparison is a **lower bound**, not an equivalent database:
it omits writer locking, full admission validation, recovery indexing and query
APIs. Do not use this result to claim Gate A or superiority over SQLite/redb.
Both cases include encoding/write/sync in the batch timing; setup/creation is
outside it. Overall elapsed time includes input generation. Run order is fixed
and cache/device scheduling can bias results.

Output includes total bytes, total duration, sample count, and observed batch
p50/p95 durations. Small sample counts have limited statistical meaning; these
are batch latencies, not per-event latency percentiles. Recovery duration is a
separate field. Defaults are 4,096 events and batches of 256; bounds are
1..=1,000,000 events and 1..=4,096 events per batch.

## Reproducible measurement protocol

For an exploratory foundation run:

1. Record revision, dirty-tree status, lockfile, `rustc --version --verbose`,
   Cargo version, target triple, profile and all arguments.
2. Record CPU/model/core allocation, RAM, OS/kernel, power/thermal settings,
   background load and whether execution is native. On Jetson include module,
   JetPack/L4T, userspace, power mode, clocks and thermal state.
3. Use the release profile; separate compilation and warm-up from recorded
   application intervals. Repeat runs, preserve every raw row, and note order/
   cache effects rather than publishing one favorable number.
4. Check correctness failures and checksums before interpreting timing. Describe
   the operations each baseline omits and all data retained in memory.
5. Store local generated reports in the ignored `benchmark-results` directory.
   Publish only reviewed non-sensitive reports with enough metadata to reproduce.

The foundation output supplies only a subset of this metadata; it is the
experimenter's responsibility to record the rest. Do not infer hardware,
compiler or durability information from an `ns/op` value.

Before a gate-quality benchmark, extend the appropriate existing harness to
capture per-operation distributions or a justified statistical equivalent,
explicit warm/cold runs, repetitions/confidence, and the relevant resource costs.
Predeclare datasets, comparisons, thresholds, allowed regressions and analysis.
Do not retune acceptance criteria after seeing results without a recorded revision.

### Equivalent comparisons

Simple incumbents come first: direct Vec/HashMap lower bounds, a validated append
log/reference executor, and SQLite/redb/Fjall only where their semantics match.
Heavier external competitors are optional and workload-specific, not current
dependencies. Never compare an in-memory unvalidated loop to another engine's
disk-synchronized transaction and call the difference a database speedup.

Gate reports must include p50/p95/p99/p99.9 where meaningful; throughput;
bytes/entity and bytes/event; **all** indexes, slot maps, metadata, WAL,
checkpoints and branch costs; read/decode/write amplification; bytes/blocks
touched; replay depth; fanout; and foreground effects of background work.
Report setup/allocations and total memory/storage separately from inner loops.
Include dataset entropy and precision, warm/cold state, cache policy, filesystem
and synchronization settings.

## Complete A–L program

| Workload | Planned exercise | Current status |
| --- | --- | --- |
| A | Packed current state | Small seeded lookup foundation |
| B | Event append, separately reported volatile/buffered/durable modes | Volatile and explicit OS-synchronized batch baselines |
| C | Spatial/N-dimensional local history | Future |
| D | Entity history | Linear scan/pagination foundation |
| E | Deterministic replay | Future |
| F | Branch/counterfactual forks | Future |
| G | Mixed Tzeentch fast/medium/slow/background load | Future |
| H | Billion-event history and bounded metadata growth | Future |
| I | Skew/hotspots and shard fanout | Future |
| J | Foreground latency under background pressure | Future |
| K | Cold/archive storage and lifecycle transitions | Future |
| L | Query budgets, continuations, tokenizer accounting and bounded AI output | Basic scan/result budgets have correctness tests; full benchmark future |

Native Windows x64, Linux x64 and Linux ARM64/AGX Orin reports are separate
qualification evidence. A Rust ARM64 cross-check executes no ARM64 workload and
does not validate JetPack 6.2.2 / L4T 36.5 / Ubuntu 22.04 performance or linking.
No native ARM64 benchmark result is supplied here.

## Historical engineering targets — unverified aspirations

These numbers preserve context from the source proposal. They are not
measurements, guarantees, release criteria already passed, or comparable unless
the stated cache/durability semantics match.

| Operation | Source aspiration |
| --- | --- |
| Packed cached current lookup | Under 1 μs; 0.05–0.5 μs aspirational |
| Cached historical point | Approximately 1–5 μs |
| Cached local 1K-event range | Approximately 5–50 μs |
| Cold NVMe point | Approximately 50–200 μs |
| Cached replay of 10K events | Approximately 30–300 μs |
| NVMe replay of 10K events | Approximately 0.2–2 ms |
| Buffered ingestion | Initially 1–5M changes/s per CPU group; 10M+ stretch, **not durable throughput** |
| Foreground global locks | Effectively zero in the intended ownership design |
| 100M-change compact history | Approximately 1–2.5 GB, highly entropy/representation-dependent |

## Gates and adaptive costs

- **Gate A:** equivalent correctness/durability plus a repeatable target-workload
  advantage over simple incumbents before EKS.
- **Gate B:** held-out, time-correct knowledge/consumer benefit with complete
  provenance and measured active-memory/runtime cost before evolution.
- **Gate C:** repeatable adaptive gains after evaluation/background costs and
  regression risk, before meta-evolution.

For adaptive results include candidate-generation/evaluation CPU/RAM/I/O,
foreground tail impact, storage amplification, warm-up, promotion and rollback.
Keep static incumbents visible. Disable/remove non-winning complexity instead
of hiding it in averages. See the [roadmap](ROADMAP.md#gates).
