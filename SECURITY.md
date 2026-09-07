# Security policy

## Report privately

Email **willaveryvy@gmail.com** with the subject `Temnion security report`.
Include the affected revision, target/toolchain, impact, and a minimal,
non-sensitive reproduction. Do not post exploit details, credentials, private
recordings, or personal data in public issues. Coordinate disclosure with the
maintainer; no response-time or remediation SLA is promised.

## Current scope

Temnion is a pre-production, volatile Rust foundation, not a hardened database
service. There is no daemon, network listener, authentication service, disk
format parser, plugin host, or durable storage in this deliverable. The active
development line is the remediation target; no released-version support window
or production security certification is established.

The current API forbids unsafe project code and uses checked identifiers,
bounded record counts and scan/result budgets. Those measures are not a security
sandbox or a complete memory quota. Generic payloads may allocate independently,
capacities are caller-configured, and this library does not isolate mutually
untrusted tenants. Do not use it as the sole copy of important data or execute
untrusted native code in its process.

## Required boundaries for later releases

These are architecture requirements, **not implemented security features**:

- No remote listener by default. Local IPC needs OS access controls; optional
  remote access needs authentication, authorization, TLS, and quotas.
- Validate framed input, lengths, versions, checksums, identifiers and resource
  requests before allocation or execution. All frontends must enforce the same
  policy; MCP and Studio must not create bypasses.
- Separate control/model/UI dependencies from the storage hot path. Treat
  query results, model output, stored strings and webview content as untrusted.
- Model and evolutionary candidates run outside the core with explicit,
  platform-appropriate isolation. An ordinary child process is not a sandbox.
- The immutable Constitution protects evidence, provenance, format validators,
  resource enforcement, authorization, retention, promotion, rollback and audits.
  Candidates cannot modify these rules.
- Durable acknowledgments require validated recovery and synchronization
  semantics. Retention defaults to exact data and no automatic deletion;
  destructive operations require authorization, reference checks and audit.

See [ARCHITECTURE](docs/ARCHITECTURE.md) and
[the core contract ADR](docs/adr/0001-foundation-contracts.md).

## CI and supply chain

Foundation CI uses hosted runners, a read-only repository token, and a
commit-pinned checkout action with credential persistence disabled. It does not
publish releases or run untrusted pull requests on Jetson/self-hosted hardware.
Dependency changes require rights/notices review; third-party software retains
its own licensing. Never include secrets or private datasets in test artifacts.
