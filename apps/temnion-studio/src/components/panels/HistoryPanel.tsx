// SPDX-License-Identifier: AGPL-3.0-only

import { useState } from "react";
import { useQuery } from "@tanstack/react-query";

import { type EventRow, listHistory } from "../../api";
import { errorText, formatBytes } from "../../utils/formatters";
import { EmptyState, EventTable, Notice, PanelHeader } from "../common";

export function TemporalPlane({ rows }: { rows: EventRow[] }) {
  if (!rows.length) return <EmptyState>Load history to plot valid time against known time.</EmptyState>;
  const valid = rows.map((row) => row.validTime);
  const known = rows.map((row) => row.knownTime);
  const validMin = Math.min(...valid);
  const validSpan = Math.max(1, Math.max(...valid) - validMin);
  const knownMin = Math.min(...known);
  const knownSpan = Math.max(1, Math.max(...known) - knownMin);
  return (
    <svg className="temporal-plane" viewBox="0 0 820 300" role="img" aria-label="Valid time by known time event plot">
      <line x1="55" y1="255" x2="790" y2="255" className="axis-line" />
      <line x1="55" y1="20" x2="55" y2="255" className="axis-line" />
      <text x="690" y="285" className="axis-label">valid time →</text>
      <text x="12" y="18" className="axis-label">known ↑</text>
      {rows.map((row) => {
        const x = 65 + ((row.validTime - validMin) / validSpan) * 710;
        const y = 245 - ((row.knownTime - knownMin) / knownSpan) * 210;
        return (
          <g key={row.eventId || row.sequence}>
            <circle cx={x} cy={y} r="6" className="event-dot" />
            <title>{`seq ${row.sequence}; entity ${row.entity}; valid ${row.validTime}; known ${row.knownTime}`}</title>
          </g>
        );
      })}
    </svg>
  );
}

export function HistoryPanel({ connected, eventCount }: { connected: boolean; eventCount: number }) {
  const [maxRows, setMaxRows] = useState(100);
  const history = useQuery({
    queryKey: ["history", maxRows, eventCount],
    queryFn: () => listHistory(maxRows),
    enabled: connected,
  });
  return (
    <section className="view-panel active">
      <PanelHeader
        title="Bi-temporal history"
        subtitle="A bounded native scan plotted on independent valid-time and known-time axes."
        action={
          <label className="budget-label">
            Rows
            <input className="num-input" min={1} max={1_000} type="number" value={maxRows} onChange={(event) => setMaxRows(Number(event.target.value))} />
          </label>
        }
      />
      {!connected && <Notice>Connect a database to load durable history.</Notice>}
      {history.error && <Notice kind="error">{errorText(history.error)}</Notice>}
      <div className="glass-card plane-card">
        <TemporalPlane rows={history.data?.rows ?? []} />
      </div>
      <div className="results-container glass-card">
        <div className="results-header">
          <div className="results-stats">
            <span><strong>{history.data?.rows.length ?? 0}</strong> rows</span>
            <span>•</span>
            <span><strong>{history.data?.eventsScanned ?? 0}</strong> scanned</span>
            <span>•</span>
            <span><strong>{formatBytes(history.data?.bytesRead ?? 0)}</strong> read</span>
            {history.data?.hasMore && <span className="badge badge-warning">More rows available</span>}
          </div>
        </div>
        <EventTable rows={history.data?.rows ?? []} showPayload />
      </div>
    </section>
  );
}
