# ADR 0011: SQL compatibility frontend lowering into canonical query IR

Status: accepted implementation contract for M22 (SQL compatibility).

## Problem and Context

While TemQL provides native temporal syntax and Compact Tem offers terse shorthand for URIs and CLI operations, relational SQL is the lingua franca of data analysts, BI tooling, and developer ecosystems.

Rather than creating a separate SQL execution engine or adding heavy SQL parser dependencies that inflate compile times and bloat storage hot paths:
1. **Unified Semantic IR:** SQL queries (`SELECT ... FROM ... WHERE ... [LIMIT n]`) must lower directly into Temnion's existing canonical typed query IR (`LogicalPlan::Scan`), guaranteeing bit-for-bit equivalence with TemQL and Compact Tem.
2. **First-Class Bi-Temporal Relational Predicates:** Standard SQL clauses must naturally express bi-temporal slices:
   - `entity = '#shard:slot:gen'` or `entity = 'shard:slot:gen'` identifies target entities.
   - `valid_time >= t1 AND valid_time < t2` maps to `valid_range`.
   - `known_as_of = tk` or `known_time <= tk` maps to `known_as_of` snapshot cutoffs.
   - Relational value filters (`health > 50`, `position == 10`, `flag != false`) map to `filter` expressions (`Expr::Binary`).
   - `LIMIT n` maps to query truncation limits.
3. **Decoupled Architecture:** Pure safe Rust without heavyweight external SQL grammar dependencies. Zero overhead added to storage or WAL paths.

## Invariants and Guarantees

1. **Exact Canonical Equivalence:** Lowering any equivalent query written in TemQL, Compact Tem, or SQL produces identical `LogicalPlan` and physical `PhysicalPlan` execution trees.
2. **Transparent Frontend Routing:** TNP (`QueryFormat::Sql`), local IPC, safe C ABI, Arrow Flight (`do_get`), MCP (`query`, `explain`), and CLI (`tem query`, `tem explain`) automatically detect and route standard SQL queries to `parse_sql`.
3. **Strict Safe Code Mandate:** The parser enforces `#![forbid(unsafe_code)]` with zero warnings under `-D warnings`.

## Architecture and Data Structures

### Parser Implementation (`crates/temnion-query/src/lib.rs`)

- `pub fn parse_sql(input: &str) -> Result<LogicalPlan, QueryError>`:
  - Extracts projection list (`SELECT ...`).
  - Validates table source (`FROM temnion`).
  - Parses compound `WHERE` predicates separated by `AND` (respecting quoted strings).
  - Categorizes clauses into:
    - Entity ID specification (`entity = '...'`).
    - Valid time range bounds (`valid_time >= ...`, `valid_time < ...`).
    - Known-as-of temporal snapshot cutoff (`known_as_of = ...`, `known_time <= ...`).
    - Relational value predicates (`field op literal`).
  - Parses optional `LIMIT n` clause.
- `split_sql_and(s: &str) -> Vec<String>`:
  - Tokenizes top-level conjunctions while preserving quoted string literals.
- `parse_sql_expr(s: &str) -> Result<Expr, QueryError>`:
  - Parses relational comparisons (`=`, `==`, `!=`, `<>`, `<=`, `>=`, `<`, `>`) into canonical `Expr::Binary` AST nodes.

### Service Layer Integration

- **TNP (`temnion-protocol`):**
  - Added `QueryFormat::Sql = 3` to wire protocol framing.
  - Negotiates `"sql"` capability in `DescribeResponse`.
  - C ABI `temnion_c_query_execute` seamlessly routes queries starting with `select` to `parse_sql`.
- **Arrow Flight (`temnion-flight`):**
  - `FlightService::do_get` detects `select` query strings in `Ticket` descriptors and lowers via `parse_sql`.
- **Model Context Protocol (`temnion-mcp`):**
  - MCP `query` and `explain` tool handlers execute SQL queries against opened databases.
- **CLI (`temnion-cli`):**
  - `tem query <dir> <sql>` and `tem explain <sql>` execute and explain SQL queries directly.
  - `tem describe` advertises `"sql": true`.
