// SPDX-License-Identifier: AGPL-3.0-only

import React, { useState } from "react";
import type { NewTimeOptions } from "../../api";
import { Notice } from "../common/Notice";

export interface CreateTimeModalProps {
  isOpen: boolean;
  activeDatabase: string;
  onClose: () => void;
  onCreate: (opts: NewTimeOptions) => void;
}

export function CreateTimeModal({
  isOpen,
  activeDatabase,
  onClose,
  onCreate,
}: CreateTimeModalProps) {
  if (!isOpen) return null;

  const [name, setName] = useState("");
  const [clockType, setClockType] = useState<"physical-utc" | "lamport-dag" | "hybrid-vector">("physical-utc");
  const [resolution, setResolution] = useState("1 ns (UTC wall-clock)");
  const [description, setDescription] = useState("");
  const [asOfTimestamp, setAsOfTimestamp] = useState("");
  const [error, setError] = useState<string | null>(null);

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!name.trim()) {
      setError("Time horizon name is required.");
      return;
    }
    onCreate({
      databaseName: activeDatabase,
      name: name.trim(),
      clockType,
      resolution,
      description: description.trim() || `Temporal plane ${name.trim()}`,
      asOfTimestamp: asOfTimestamp.trim() || undefined,
    });
  };

  return (
    <div className="object-modal-overlay" onClick={onClose}>
      <div className="object-modal-card" onClick={(e) => e.stopPropagation()}>
        <div className="object-modal-header">
          <h3><span>◴</span> New Time Horizon / Checkpoint</h3>
          <button className="object-modal-close-btn" onClick={onClose} type="button">×</button>
        </div>
        <form onSubmit={handleSubmit}>
          <div className="object-modal-body">
            {error && <Notice kind="error">{error}</Notice>}
            <div className="form-group">
              <label className="form-label">Time Horizon Name</label>
              <input
                className="form-input"
                type="text"
                autoFocus
                placeholder="e.g. Physical Nanosecond Grid, Audit Checkpoint"
                value={name}
                onChange={(e) => {
                  setName(e.target.value);
                  setError(null);
                }}
              />
            </div>

            <div className="form-group">
              <label className="form-label">Clock Coordinate Dimension</label>
              <select
                className="form-input"
                value={clockType}
                onChange={(e) => {
                  const val = e.target.value as "physical-utc" | "lamport-dag" | "hybrid-vector";
                  setClockType(val);
                  if (val === "physical-utc") setResolution("1 ns (UTC wall-clock)");
                  else if (val === "lamport-dag") setResolution("Lamport Monotonic Tick");
                  else setResolution("Epoch Vector Marker");
                }}
              >
                <option value="physical-utc">Physical Wall-Clock (UTC Nanoseconds)</option>
                <option value="lamport-dag">Logical Plane (Lamport Sequence & Causal DAG)</option>
                <option value="hybrid-vector">Hybrid Multi-Dimensional Vector</option>
              </select>
            </div>

            <div className="form-group">
              <label className="form-label">Temporal Resolution</label>
              <input
                className="form-input"
                type="text"
                value={resolution}
                onChange={(e) => setResolution(e.target.value)}
              />
            </div>

            <div className="form-group">
              <label className="form-label">Point-in-Time Flashback Target (Optional)</label>
              <input
                className="form-input"
                type="text"
                placeholder="2026-09-08T00:00:00Z"
                value={asOfTimestamp}
                onChange={(e) => setAsOfTimestamp(e.target.value)}
              />
              <div className="form-hint">Enables zero-copy AS OF SYSTEM_TIME historical flashback.</div>
            </div>

            <div className="form-group">
              <label className="form-label">Description</label>
              <input
                className="form-input"
                type="text"
                placeholder="Temporal semantics and boundary"
                value={description}
                onChange={(e) => setDescription(e.target.value)}
              />
            </div>
          </div>

          <div className="object-modal-footer">
            <button className="btn btn-secondary btn-sm" type="button" onClick={onClose}>
              Cancel
            </button>
            <button className="btn btn-primary btn-sm" type="submit">
              Create Time Horizon
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
