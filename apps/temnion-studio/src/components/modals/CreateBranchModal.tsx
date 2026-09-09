// SPDX-License-Identifier: AGPL-3.0-only

import React, { useState } from "react";
import type { BranchObject, NewBranchOptions } from "../../api";
import { Notice } from "../common/Notice";

export interface CreateBranchModalProps {
  isOpen: boolean;
  activeDatabase: string;
  existingBranches: BranchObject[];
  onClose: () => void;
  onCreate: (opts: NewBranchOptions) => void;
}

export function CreateBranchModal({
  isOpen,
  activeDatabase,
  existingBranches,
  onClose,
  onCreate,
}: CreateBranchModalProps) {
  if (!isOpen) return null;

  const [name, setName] = useState("");
  const [parentId, setParentId] = useState<number>(existingBranches[0]?.id ?? 1);
  const [forkSequence, setForkSequence] = useState<number>(10);
  const [error, setError] = useState<string | null>(null);

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!name.trim()) {
      setError("Branch name is required.");
      return;
    }
    onCreate({
      databaseName: activeDatabase,
      name: name.trim(),
      parentId,
      forkSequence,
      lifecycle: "Active",
    });
  };

  return (
    <div className="object-modal-overlay" onClick={onClose}>
      <div className="object-modal-card" onClick={(e) => e.stopPropagation()}>
        <div className="object-modal-header">
          <h3><span>⑂</span> New Temporal Branch</h3>
          <button className="object-modal-close-btn" onClick={onClose} type="button">×</button>
        </div>
        <form onSubmit={handleSubmit}>
          <div className="object-modal-body">
            {error && <Notice kind="error">{error}</Notice>}
            <div className="form-group">
              <label className="form-label">Branch Name</label>
              <input
                className="form-input"
                type="text"
                autoFocus
                placeholder="e.g. experiment/high-frequency, hotfix-causal"
                value={name}
                onChange={(e) => {
                  setName(e.target.value);
                  setError(null);
                }}
              />
              <div className="form-hint">Bi-temporal fork identifier for concurrent causal timelines.</div>
            </div>

            <div className="form-group">
              <label className="form-label">Parent Branch</label>
              <select
                className="form-input"
                value={parentId}
                onChange={(e) => setParentId(Number(e.target.value))}
              >
                {existingBranches.map((b) => (
                  <option key={b.id} value={b.id}>
                    {b.name} (id: {b.id})
                  </option>
                ))}
              </select>
            </div>

            <div className="form-group">
              <label className="form-label">Fork Sequence Number</label>
              <input
                className="form-input"
                type="number"
                min={0}
                value={forkSequence}
                onChange={(e) => setForkSequence(Number(e.target.value))}
              />
              <div className="form-hint">The Lamport sequence point where this branch diverges.</div>
            </div>
          </div>

          <div className="object-modal-footer">
            <button className="btn btn-secondary btn-sm" type="button" onClick={onClose}>
              Cancel
            </button>
            <button className="btn btn-primary btn-sm" type="submit">
              Create Branch
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
