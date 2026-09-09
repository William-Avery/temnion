// SPDX-License-Identifier: AGPL-3.0-only

import { useState } from "react";
import { useQuery } from "@tanstack/react-query";

import { listEntities, listSchemas } from "../../api";
import { PanelHeader } from "../common";

export function SchemaPanel() {
  const schemasQuery = useQuery({ queryKey: ["schemas"], queryFn: listSchemas });
  const entitiesQuery = useQuery({ queryKey: ["entities"], queryFn: listEntities });
  const [selectedSchemaId, setSelectedSchemaId] = useState<number>(1);

  const schemas = schemasQuery.data ?? [];
  const entities = entitiesQuery.data ?? [];
  const selectedSchema = schemas.find((s) => s.id === selectedSchemaId) ?? schemas[0];

  return (
    <section className="view-panel active">
      <PanelHeader
        title="Schema & Entity Catalog"
        subtitle="Authoritative schemas, field layouts, index coverage, and registered entity slots."
      />

      {/* Schema Cards Grid */}
      <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(240px, 1fr))", gap: "12px", marginBottom: "16px" }}>
        {schemas.map((schema) => (
          <div
            key={schema.id}
            className={`glass-card ${selectedSchemaId === schema.id ? "active-conn" : ""}`}
            style={{ padding: "14px", cursor: "pointer", border: selectedSchemaId === schema.id ? "1px solid #38bdf8" : undefined }}
            onClick={() => setSelectedSchemaId(schema.id)}
          >
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "8px" }}>
              <span className="badge badge-neutral" style={{ fontWeight: "bold" }}>Schema #{schema.id}</span>
              <span className="badge badge-success">{schema.eventCount} events</span>
            </div>
            <div style={{ fontSize: "15px", fontWeight: "bold", color: "#e2e8f0", marginBottom: "4px" }}>
              {schema.name}
            </div>
            <div style={{ fontSize: "12px", color: "#94a3b8", lineHeight: "1.4" }}>
              {schema.description}
            </div>
          </div>
        ))}
      </div>

      {/* Selected Schema Fields Inspector */}
      {selectedSchema && (
        <div className="glass-card" style={{ padding: "16px", marginBottom: "16px" }}>
          <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "12px" }}>
            <div>
              <div style={{ fontSize: "16px", fontWeight: "bold", color: "#e2e8f0" }}>
                Field Layout: <span style={{ color: "#38bdf8" }}>{selectedSchema.name}</span> (Schema #{selectedSchema.id})
              </div>
              <div style={{ fontSize: "12px", color: "#94a3b8" }}>
                Canonical Clock: Clock #{selectedSchema.clockId} · Total Fields: {selectedSchema.fields.length}
              </div>
            </div>
            <span className="badge badge-neutral">Arrow Columnar Compatible</span>
          </div>

          <table className="events-table">
            <thead>
              <tr>
                <th>Field Name</th>
                <th>Logical Data Type</th>
                <th>Indexing</th>
                <th>Storage Layout</th>
              </tr>
            </thead>
            <tbody>
              {selectedSchema.fields.map((f) => (
                <tr key={f.name}>
                  <td style={{ fontFamily: "monospace", color: "#e2e8f0", fontWeight: "bold" }}>{f.name}</td>
                  <td><span className="badge badge-neutral">{f.type}</span></td>
                  <td>
                    {f.indexed ? (
                      <span className="badge badge-success">Indexed (ZoneMap + Bloom)</span>
                    ) : (
                      <span className="badge badge-neutral">Direct Scan</span>
                    )}
                  </td>
                  <td style={{ fontSize: "12px", color: "#94a3b8" }}>Fixed-Width Bitpacked</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {/* Entity Slot Registry */}
      <div className="glass-card" style={{ padding: "16px" }}>
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "12px" }}>
          <div>
            <div style={{ fontSize: "15px", fontWeight: "bold", color: "#e2e8f0" }}>
              Registered Entity Slots
            </div>
            <div style={{ fontSize: "12px", color: "#94a3b8" }}>
              Authoritative shard-local entity mapping with temporal sequence counters.
            </div>
          </div>
          <span className="badge badge-neutral">{entities.length} active slots</span>
        </div>

        <table className="events-table">
          <thead>
            <tr>
              <th>Entity ID</th>
              <th>Shard ID</th>
              <th>Slot Index</th>
              <th>Generation</th>
              <th>Schema Assigned</th>
              <th>Event Count</th>
              <th>Last Valid Time</th>
            </tr>
          </thead>
          <tbody>
            {entities.map((e) => (
              <tr key={e.id}>
                <td style={{ fontFamily: "monospace", color: "#38bdf8", fontWeight: "bold" }}>{e.id}</td>
                <td>Shard {e.shard}</td>
                <td>Slot #{e.slot}</td>
                <td>Gen {e.generation}</td>
                <td><span className="badge badge-neutral">Schema #{e.schemaId}</span></td>
                <td>{e.totalEvents} events</td>
                <td style={{ fontFamily: "monospace" }}>{e.lastValidTime} ticks</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  );
}
