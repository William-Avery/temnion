// SPDX-License-Identifier: AGPL-3.0-only

import type { EngineStatus } from "../../api";
import { formatBytes } from "../../utils/formatters";
import { Notice, PanelHeader } from "../common";

export function MetricsPanel({ status }: { status: EngineStatus }) {
  return (
    <section className="view-panel active">
      <PanelHeader
        title="Storage & Capabilities"
        subtitle="Authoritative store metrics, WAL retirement status, manifest metadata, and advertised capabilities."
      />
      <div className="metrics-grid">
        <div className="glass-card metric-card">
          <div className="metric-val">{status.eventCount}</div>
          <div className="metric-label">Durable events</div>
        </div>
        <div className="glass-card metric-card">
          <div className="metric-val">{formatBytes(status.walBytes)}</div>
          <div className="metric-label">Active WAL prefix</div>
        </div>
        <div className="glass-card metric-card">
          <div className="metric-val">{status.summaryBlocks}</div>
          <div className="metric-label">Retired Segments (TSF)</div>
        </div>
        <div className="glass-card metric-card">
          <div className="metric-val">{status.maxRows}</div>
          <div className="metric-label">Maximum rows per view</div>
        </div>
      </div>

      <div className="glass-card capabilities-card">
        <div className="card-title">Engine Capability Surface</div>
        <div className="capability-list">
          {status.capabilities.map((capability) => (
            <span className="badge badge-neutral" key={capability}>
              {capability}
            </span>
          ))}
        </div>
        <dl className="identity-grid">
          <div><dt>Database ID</dt><dd>{status.databaseId ?? "—"}</dd></div>
          <div><dt>Source ID</dt><dd>{status.source ?? "—"}</dd></div>
          <div><dt>Authoritative Epoch</dt><dd>{status.epoch ?? "—"}</dd></div>
          <div><dt>Memory Safety</dt><dd>forbid(unsafe_code)</dd></div>
        </dl>
      </div>

      <Notice>
        WAL retirement is enabled and guarded by SegmentManifest (`segments/manifest.bin`). Sealed frames are retired into immutable `.tsf` segments while preserving active reference holds.
      </Notice>
    </section>
  );
}
