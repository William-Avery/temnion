# ADR 0010: Arrow Flight remote analytical service and Model Context Protocol (MCP) server

Status: accepted implementation contract for M20 (Arrow Flight) and M21 (Model Context Protocol / MCP).

## Problem and Context

Temnion requires clean, standardized interfaces for distributed analytical runtimes and autonomous AI agents:
1. **Remote Analytical Transport (M20):** External distributed query engines (DataFusion, DuckDB, Spark), Python/Polars runtimes, and remote workers require high-throughput bulk data streaming via Apache Arrow Flight RPC semantics rather than row-by-row serialization.
2. **AI Control Plane & Tool Calling (M21):** AI coding assistants, autonomous agents, and LLM reasoning pipelines need an official control-plane interface to inspect schemas, execute bounded queries, audit causal history, and fork timelines using the Model Context Protocol (MCP) JSON-RPC 2.0 specification.
3. **Decoupled Architecture:** As mandated by Temnion Architecture §7, networking, JSON serialization, and control-plane RPC dependencies must remain strictly decoupled from storage and execution hot paths. No remote listener is enabled by default.

Following Temnion Architecture §7, §29, and §30, the `temnion-flight` and `temnion-mcp` crates implement these interfaces under `#![forbid(unsafe_code)]`.

## Invariants and Guarantees

1. **Checksummed Flight Framing:** All remote Flight packets use binary magic `b"FLGT"`, explicit version headers, and trailing CRC32C checksums with a hard 32 MB payload upper bound (`MAX_FLIGHT_PAYLOAD`).
2. **Authenticated Session Tokens:** Remote Flight endpoints require a valid handshake token (`FlightHandshakeRequest`) before issuing tickets or dispatching data streams (`do_get`).
3. **Columnar Memory Batching:** `FlightData` packets carry Arrow-compatible `ColumnarBatch` analytical vectors with explicit header lengths and record counts.
4. **Control-Plane MCP Isolation:** `temnion-mcp` operates purely on the control plane (via stdio or line-delimited streams), protecting core engine performance.
5. **Bounded Tool Execution:** MCP tools (`query`, `explain`, `inspect`, `branch_list`, `causal_trace`) enforce explicit row limits, event scan budgets, and memory ceilings, safely terminating queries with truncation indicators if limits are reached.
6. **Self-Describing Resources & Prompts:** Exposes standardized `temnion://database/*` resource URIs and guided prompt templates (`causal-investigation`, `timeline-audit`).
7. **Strict Safe Code Mandate:** Pure safe Rust enforcing `#![forbid(unsafe_code)]` with zero compiler warnings under `-D warnings`.

## Architecture and Data Structures

### Arrow Flight Service (`temnion-flight` - M20)

- `FlightDescriptor`:
  - `Cmd(String)`: Query command string (TemQL or compact `tn:`).
  - `Path(Vec<String>)`: Hierarchical table or dataset path.
  - `None`: Unspecified descriptor.
- `Ticket`:
  - Opaque authorization token encoding query string and maximum row count.
- `FlightInfo`:
  - Schema definition byte slice, descriptor, list of `FlightEndpoint` locations with tickets, and total record/byte estimates.
- `FlightData`:
  - Binary framed chunk containing batch metadata header and columnar vector bodies (`ColumnarBatch`).
- `FlightService`:
  - `handshake`: Authenticates client credentials and returns session tokens.
  - `get_flight_info`: Plans queries and emits `FlightInfo` with endpoint tickets.
  - `do_get`: Redeems tickets and streams `FlightData` batches.
  - `do_action`: Control plane actions (`ping`, `status`).

### Model Context Protocol Server (`temnion-mcp` - M21)

- `JsonValue`: Lightweight, zero-dependency, safe JSON DOM parser and serializer supporting null, bool, number, string, array, and object types.
- `McpServer`:
  - `initialize`: Returns protocol version (`2024-11-05`), server identity (`temnion-mcp`), and advertised capabilities.
  - **Tools (`tools/list`, `tools/call`)**:
    - `query`: Executes bounded TemQL or compact `tn:` queries.
    - `explain`: Renders physical execution trees and cost estimates.
    - `inspect`: Reports WAL sequence, epoch, source ID, and storage metrics.
    - `branch_list`: Lists active timeline branches and lifecycles.
    - `causal_trace`: Traces causal ancestry and effect cones.
  - **Resources (`resources/list`, `resources/read`)**:
    - `temnion://database/capabilities`: Engine capabilities JSON.
    - `temnion://database/branches`: Active branch manifest details.
    - `temnion://database/summaries`: Zone maps and Bloom filter index stats.
  - **Prompts (`prompts/list`, `prompts/get`)**:
    - `causal-investigation`: Investigative template for anomaly root-cause analysis.
    - `timeline-audit`: Template for timeline divergence audits.
  - `run_stdio`: Runs line-delimited JSON-RPC loop over standard input and output streams.

## Verification and Testing

1. **Flight Descriptors & Tickets:** Roundtrip encoding/decoding tests for all descriptor types and tickets.
2. **Flight Framing & Integrity:** Bit-exact verification of CRC32C checksums, truncation detection, and magic validation.
3. **Flight Service Pipeline:** End-to-end authentication, schema introspection (`get_flight_info`), and streaming columnar retrieval (`do_get`).
4. **MCP Protocol Compliance:** Initialization handshake, ping liveness, tool dispatch, resource reading, and prompt generation.
5. **CLI Integration:** Verification of `tem mcp` command and capability advertisements (`"mcp": true`, `"flight"`, `"mcp"`).
