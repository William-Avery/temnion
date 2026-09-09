// SPDX-License-Identifier: AGPL-3.0-only

import React, { useState } from "react";
import type { NewQueryOptions } from "../../api";
import { Notice } from "../common/Notice";
import { QueryEditor } from "../../QueryEditor";

export interface CreateQueryModalProps {
  isOpen: boolean;
  activeDatabase: string;
  onClose: () => void;
  onCreate: (opts: NewQueryOptions) => void;
}

export function CreateQueryModal({
  isOpen,
  activeDatabase,
  onClose,
  onCreate,
}: CreateQueryModalProps) {
  if (!isOpen) return null;

  const [name, setName] = useState("");
  const [format, setFormat] = useState<"sql" | "temql">("sql");
  const [queryText, setQueryText] = useState(
    "SELECT entity, schema, valid_time, known_time, sequence\nFROM temnion\nLIMIT 50"
  );
  const [error, setError] = useState<string | null>(null);

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!name.trim()) {
      setError("Query title is required.");
      return;
    }
    onCreate({
      databaseName: activeDatabase,
      name: name.trim(),
      format,
      queryText,
    });
  };

  return (
    <div className="object-modal-overlay" onClick={onClose}>
      <div className="object-modal-card" onClick={(e) => e.stopPropagation()}>
        <div className="object-modal-header">
          <h3><span>⚡</span> New Saved Query</h3>
          <button className="object-modal-close-btn" onClick={onClose} type="button">×</button>
        </div>
        <form onSubmit={handleSubmit}>
          <div className="object-modal-body">
            {error && <Notice kind="error">{error}</Notice>}
            <div style={{ display: "flex", gap: "12px" }}>
              <div className="form-group" style={{ flex: 1 }}>
                <label className="form-label">Query Title</label>
                <input
                  className="form-input"
                  type="text"
                  autoFocus
                  placeholder="e.g. Anomaly Detection Scan"
                  value={name}
                  onChange={(e) => {
                    setName(e.target.value);
                    setError(null);
                  }}
                />
              </div>
              <div className="form-group" style={{ width: "120px" }}>
                <label className="form-label">Language</label>
                <select
                  className="form-input"
                  value={format}
                  onChange={(e) => {
                    const next = e.target.value as "sql" | "temql";
                    setFormat(next);
                    if (next === "temql") {
                      setQueryText("FROM temnion\nWHERE sequence > 0\nSELECT entity, schema, valid_time, known_time, sequence\nLIMIT 25");
                    } else {
                      setQueryText("SELECT entity, schema, valid_time, known_time, sequence\nFROM temnion\nLIMIT 50");
                    }
                  }}
                >
                  <option value="sql">SQL</option>
                  <option value="temql">TemQL</option>
                </select>
              </div>
            </div>

            <div className="form-group">
              <label className="form-label">Query Definition</label>
              <QueryEditor
                ariaLabel="Saved query definition"
                className="query-code-editor--compact"
                language={format}
                onChange={setQueryText}
                value={queryText}
              />
            </div>
          </div>

          <div className="object-modal-footer">
            <button className="btn btn-secondary btn-sm" type="button" onClick={onClose}>
              Cancel
            </button>
            <button className="btn btn-primary btn-sm" type="submit">
              Save Query
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
