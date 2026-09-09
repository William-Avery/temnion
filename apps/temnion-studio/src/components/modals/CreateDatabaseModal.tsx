// SPDX-License-Identifier: AGPL-3.0-only

import React, { useState } from "react";
import type { ConnectionProfile, CreateDatabaseOptions } from "../../api";
import { Notice } from "../common/Notice";

export interface CreateDatabaseModalProps {
  isOpen: boolean;
  connections: ConnectionProfile[];
  initialConnectionId: string;
  onClose: () => void;
  onCreate: (opts: CreateDatabaseOptions) => void;
}

export function CreateDatabaseModal({
  isOpen,
  connections,
  initialConnectionId,
  onClose,
  onCreate,
}: CreateDatabaseModalProps) {
  if (!isOpen) return null;

  const [name, setName] = useState("");
  const [connId, setConnId] = useState(initialConnectionId);
  const [clockProfile, setClockProfile] = useState("Canonical Clock #1 (Physical UTC + Lamport + Causal DAG)");
  const [template, setTemplate] = useState("Industrial IoT & Telemetry");
  const [storageTarget, setStorageTarget] = useState<"managed" | "embedded">("managed");
  const [path, setPath] = useState("");
  const [error, setError] = useState<string | null>(null);

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!name.trim()) {
      setError("Database name is required.");
      return;
    }
    onCreate({
      name: name.trim(),
      connectionId: connId,
      clockProfile,
      template,
      storageTarget,
      path: storageTarget === "embedded" ? path : undefined,
    });
  };

  return (
    <div className="object-modal-overlay" onClick={onClose}>
      <div className="object-modal-card" onClick={(e) => e.stopPropagation()}>
        <div className="object-modal-header">
          <h3><span>🗄️</span> New Database</h3>
          <button className="object-modal-close-btn" onClick={onClose} type="button">×</button>
        </div>
        <form onSubmit={handleSubmit}>
          <div className="object-modal-body">
            {error && <Notice kind="error">{error}</Notice>}
            <div className="form-group">
              <label className="form-label">Database Identifier</label>
              <input
                className="form-input"
                type="text"
                autoFocus
                placeholder="e.g. sensor_stream, production_ledger"
                value={name}
                onChange={(e) => {
                  setName(e.target.value);
                  setError(null);
                }}
              />
              <div className="form-hint">Unique alphanumeric catalog name (snake_case recommended).</div>
            </div>

            <div className="form-group">
              <label className="form-label">Target Connection</label>
              <select
                className="form-input"
                value={connId}
                onChange={(e) => setConnId(e.target.value)}
              >
                {connections.map((c) => (
                  <option key={c.id} value={c.id}>
                    {c.name} ({c.host}:{c.tnpPort})
                  </option>
                ))}
              </select>
            </div>

            <div className="form-group">
              <label className="form-label">Multi-Clock Coordinate Profile</label>
              <select
                className="form-input"
                value={clockProfile}
                onChange={(e) => setClockProfile(e.target.value)}
              >
                <option value="Canonical Clock #1 (Physical UTC + Lamport + Causal DAG)">
                  Canonical Clock #1 (Physical UTC + Lamport + Causal DAG)
                </option>
                <option value="Physical Wall-Clock Only (UTC Nanoseconds)">
                  Physical Wall-Clock Only (UTC Nanoseconds)
                </option>
                <option value="Logical Lamport Only (Deterministic Distributed Ordering)">
                  Logical Lamport Only (Deterministic Distributed Ordering)
                </option>
                <option value="Multi-Dimensional Vector Clock (Multi-Region Cluster)">
                  Multi-Dimensional Vector Clock (Multi-Region Cluster)
                </option>
              </select>
              <div className="form-hint">Governs temporal axes and timestamp resolution in storage segments.</div>
            </div>

            <div className="form-group">
              <label className="form-label">Schema Archetype Template</label>
              <select
                className="form-input"
                value={template}
                onChange={(e) => setTemplate(e.target.value)}
              >
                <option value="Industrial IoT & Telemetry">Industrial IoT & Telemetry (Sensors, Turbines)</option>
                <option value="Financial Ledger & Settlement">Financial Ledger & Settlement (Double-Entry, Transfers)</option>
                <option value="System Security & Cryptographic Audit">System Security & Cryptographic Audit (Tokens, IAM)</option>
                <option value="Standard / Blank">Standard / Blank Catalog</option>
              </select>
            </div>

            <div className="form-group">
              <label className="form-label">Storage Architecture</label>
              <div style={{ display: "flex", gap: "14px", marginTop: "4px" }}>
                <label style={{ display: "flex", alignItems: "center", gap: "6px", cursor: "pointer", fontSize: "13px", color: "#e2e8f0" }}>
                  <input
                    type="radio"
                    name="storageTarget"
                    checked={storageTarget === "managed"}
                    onChange={() => setStorageTarget("managed")}
                  />
                  TNP Daemon Managed
                </label>
                <label style={{ display: "flex", alignItems: "center", gap: "6px", cursor: "pointer", fontSize: "13px", color: "#e2e8f0" }}>
                  <input
                    type="radio"
                    name="storageTarget"
                    checked={storageTarget === "embedded"}
                    onChange={() => setStorageTarget("embedded")}
                  />
                  Local Embedded Directory
                </label>
              </div>
            </div>

            {storageTarget === "embedded" && (
              <div className="form-group">
                <label className="form-label">Embedded Store Path</label>
                <input
                  className="form-input"
                  type="text"
                  placeholder="./data/my_database"
                  value={path}
                  onChange={(e) => setPath(e.target.value)}
                />
              </div>
            )}
          </div>

          <div className="object-modal-footer">
            <button className="btn btn-secondary btn-sm" type="button" onClick={onClose}>
              Cancel
            </button>
            <button className="btn btn-primary btn-sm" type="submit">
              Create Database
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
