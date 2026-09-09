// SPDX-License-Identifier: AGPL-3.0-only

import React, { useState } from "react";
import type { NewTableOptions, TableColumn } from "../../api";
import { Notice } from "../common/Notice";

export interface CreateTableModalProps {
  isOpen: boolean;
  activeDatabase: string;
  onClose: () => void;
  onCreate: (opts: NewTableOptions) => void;
}

export function CreateTableModal({
  isOpen,
  activeDatabase,
  onClose,
  onCreate,
}: CreateTableModalProps) {
  if (!isOpen) return null;

  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [fields, setFields] = useState<TableColumn[]>([
    { name: "id", type: "Utf8", indexed: true },
    { name: "timestamp_utc", type: "Timestamp", indexed: true },
    { name: "value", type: "Float64", indexed: false },
    { name: "status", type: "Utf8", indexed: true },
  ]);
  const [error, setError] = useState<string | null>(null);

  const handleAddField = () => {
    setFields((prev) => [
      ...prev,
      { name: `field_${prev.length + 1}`, type: "Utf8", indexed: false },
    ]);
  };

  const handleRemoveField = (index: number) => {
    if (fields.length <= 1) return;
    setFields((prev) => prev.filter((_, i) => i !== index));
  };

  const handleFieldChange = (index: number, key: keyof TableColumn, val: any) => {
    setFields((prev) =>
      prev.map((f, i) => (i === index ? { ...f, [key]: val } : f))
    );
  };

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!name.trim()) {
      setError("Table name is required.");
      return;
    }
    onCreate({
      databaseName: activeDatabase,
      name: name.trim(),
      description: description.trim() || `Table ${name.trim()}`,
      fields,
    });
  };

  return (
    <div className="object-modal-overlay" onClick={onClose}>
      <div className="object-modal-card" style={{ maxWidth: "640px" }} onClick={(e) => e.stopPropagation()}>
        <div className="object-modal-header">
          <h3><span>⊞</span> New Table</h3>
          <button className="object-modal-close-btn" onClick={onClose} type="button">×</button>
        </div>
        <form onSubmit={handleSubmit}>
          <div className="object-modal-body">
            {error && <Notice kind="error">{error}</Notice>}
            <div style={{ display: "flex", gap: "12px" }}>
              <div className="form-group" style={{ flex: 1 }}>
                <label className="form-label">Table Name</label>
                <input
                  className="form-input"
                  type="text"
                  autoFocus
                  placeholder="e.g. TurbineVibrations"
                  value={name}
                  onChange={(e) => {
                    setName(e.target.value);
                    setError(null);
                  }}
                />
              </div>
              <div className="form-group" style={{ width: "160px" }}>
                <label className="form-label">Target Database</label>
                <div style={{ padding: "8px 12px", background: "rgba(255,255,255,0.04)", borderRadius: "var(--radius-sm)", color: "#a5b4fc", fontWeight: 600, fontSize: "12px", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                  🗄️ {activeDatabase}
                </div>
              </div>
            </div>

            <div className="form-group">
              <label className="form-label">Description</label>
              <input
                className="form-input"
                type="text"
                placeholder="Brief table semantics and purpose"
                value={description}
                onChange={(e) => setDescription(e.target.value)}
              />
            </div>

            <div className="fields-editor-container">
              <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
                <label className="form-label" style={{ margin: 0 }}>Column Definitions</label>
                <button
                  className="btn btn-secondary btn-sm"
                  type="button"
                  style={{ padding: "2px 8px", fontSize: "11px" }}
                  onClick={handleAddField}
                >
                  + Add Column
                </button>
              </div>
              <div style={{ maxHeight: "200px", overflowY: "auto", border: "1px solid rgba(255,255,255,0.08)", borderRadius: "var(--radius-sm)" }}>
                <table className="fields-editor-table">
                  <thead>
                    <tr>
                      <th>Column Name</th>
                      <th>Data Type</th>
                      <th style={{ textAlign: "center", width: "70px" }}>Indexed</th>
                      <th style={{ width: "40px" }}></th>
                    </tr>
                  </thead>
                  <tbody>
                    {fields.map((f, idx) => (
                      <tr key={idx}>
                        <td>
                          <input
                            className="form-input"
                            style={{ padding: "4px 8px", fontSize: "12px" }}
                            value={f.name}
                            onChange={(e) => handleFieldChange(idx, "name", e.target.value)}
                          />
                        </td>
                        <td>
                          <select
                            className="form-input"
                            style={{ padding: "4px 8px", fontSize: "12px" }}
                            value={f.type}
                            onChange={(e) => handleFieldChange(idx, "type", e.target.value)}
                          >
                            <option value="Utf8">Utf8 (String)</option>
                            <option value="Float64">Float64 (Float)</option>
                            <option value="Int64">Int64 (Integer)</option>
                            <option value="Decimal128">Decimal128 (Currency)</option>
                            <option value="Boolean">Boolean</option>
                            <option value="Timestamp">Timestamp</option>
                          </select>
                        </td>
                        <td style={{ textAlign: "center" }}>
                          <input
                            type="checkbox"
                            checked={f.indexed}
                            onChange={(e) => handleFieldChange(idx, "indexed", e.target.checked)}
                          />
                        </td>
                        <td style={{ textAlign: "center" }}>
                          {fields.length > 1 && (
                            <button
                              type="button"
                              className="tree-item-del-btn"
                              onClick={() => handleRemoveField(idx)}
                              title="Delete column"
                            >
                              🗑
                            </button>
                          )}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </div>
          </div>

          <div className="object-modal-footer">
            <button className="btn btn-secondary btn-sm" type="button" onClick={onClose}>
              Cancel
            </button>
            <button className="btn btn-primary btn-sm" type="submit">
              Create Table
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
