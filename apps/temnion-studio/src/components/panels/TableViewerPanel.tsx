// SPDX-License-Identifier: AGPL-3.0-only

import { useQuery } from "@tanstack/react-query";
import { type EventRow, listHistory, listSchemas } from "../../api";
import { EventTable } from "../common";

export interface TableViewerPanelProps {
  tableName: string;
  onRunQuery: (query: string) => void;
}

export function TableViewerPanel({
  tableName,
  onRunQuery,
}: TableViewerPanelProps) {
  const schemasQuery = useQuery({ queryKey: ["schemas"], queryFn: listSchemas });
  const schemas = schemasQuery.data ?? [];
  const schema = schemas.find((s) => s.name.toLowerCase() === tableName.toLowerCase()) ?? schemas[0];

  const historyQuery = useQuery({ queryKey: ["history", 50], queryFn: () => listHistory(50) });
  const historyRows: EventRow[] = historyQuery.data?.rows ?? [];
  const filteredRows = historyRows.filter(
    (r: EventRow) => (schema ? r.schema === schema.id : true)
  );

  return (
    <section className="view-panel active">
      <div className="table-viewer-toolbar">
        <div style={{ display: "flex", alignItems: "center", gap: "10px" }}>
          <span style={{ fontSize: "18px" }}>⊞</span>
          <div>
            <div style={{ fontSize: "15px", fontWeight: "bold", color: "#f8fafc" }}>
              Table: <span style={{ color: "#38bdf8" }}>{schema?.name ?? tableName}</span>
            </div>
            <div style={{ fontSize: "11px", color: "#94a3b8" }}>
              {schema?.description ?? "Bi-temporal columnar event entity table"}
            </div>
          </div>
        </div>
        <div style={{ display: "flex", gap: "8px" }}>
          <button
            className="btn btn-primary btn-sm"
            type="button"
            onClick={() =>
              onRunQuery(`SELECT entity, schema, valid_time, known_time, sequence\nFROM temnion\nLIMIT 100`)
            }
          >
            ⚡ Open in Query Studio
          </button>
        </div>
      </div>

      {schema && (
        <div className="glass-card" style={{ padding: "14px", marginBottom: "16px" }}>
          <div
            style={{
              fontSize: "12px",
              fontWeight: "bold",
              color: "#cbd5e1",
              textTransform: "uppercase",
              letterSpacing: "0.06em",
              marginBottom: "8px",
            }}
          >
            Column Definitions & Types
          </div>
          <div style={{ display: "flex", flexWrap: "wrap", gap: "8px" }}>
            {schema.fields.map((f) => (
              <span key={f.name} className="stat-pill" style={{ padding: "4px 10px" }}>
                <strong style={{ color: "#e2e8f0" }}>{f.name}</strong>
                <span style={{ color: "#818cf8" }}>: {f.type}</span>
                {f.indexed && <span style={{ color: "#34d399", fontSize: "10px" }}>[IDX]</span>}
              </span>
            ))}
          </div>
        </div>
      )}

      <div className="glass-card" style={{ padding: "16px" }}>
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "12px" }}>
          <div style={{ fontSize: "14px", fontWeight: "600", color: "#e2e8f0" }}>
            Data Grid ({filteredRows.length > 0 ? filteredRows.length : historyRows.length} rows)
          </div>
          <span className="badge badge-neutral">Arrow Columnar Format</span>
        </div>
        <EventTable rows={filteredRows.length > 0 ? filteredRows : historyRows} showPayload={true} />
      </div>
    </section>
  );
}
