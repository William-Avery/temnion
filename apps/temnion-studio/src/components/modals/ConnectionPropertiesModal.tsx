// SPDX-License-Identifier: AGPL-3.0-only

import React, { useEffect, useState } from "react";
import { type ConnectionProfile, testConnection } from "../../api";
import { errorText } from "../../utils/formatters";

export interface ConnectionPropertiesModalProps {
  connection: ConnectionProfile | null;
  isOpen: boolean;
  onClose: () => void;
  onSave: (connection: ConnectionProfile) => void;
}

export function ConnectionPropertiesModal({
  connection,
  isOpen,
  onClose,
  onSave,
}: ConnectionPropertiesModalProps) {
  if (!isOpen || !connection) return null;

  const [form, setForm] = useState<ConnectionProfile>({ ...connection });
  const [showToken, setShowToken] = useState(false);
  const [activeTab, setActiveTab] = useState<"general" | "network" | "ssl">("general");
  const [testResult, setTestResult] = useState<{
    success: boolean;
    message: string;
    latencyMs?: number;
    serverVersion?: string;
  } | null>(null);
  const [testing, setTesting] = useState(false);

  useEffect(() => {
    setForm({ ...connection });
    setTestResult(null);
  }, [connection]);

  const handleTest = async () => {
    setTesting(true);
    setTestResult(null);
    try {
      const res = await testConnection(form);
      setTestResult({
        success: res.success,
        message: res.message,
        latencyMs: res.latencyMs,
        serverVersion: res.version,
      });
    } catch (err) {
      setTestResult({
        success: false,
        message: errorText(err),
      });
    } finally {
      setTesting(false);
    }
  };

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    const updated: ConnectionProfile = {
      ...form,
      id: form.id || `conn-${Date.now()}`,
      latencyMs: testResult?.latencyMs ?? form.latencyMs ?? 0.38,
      serverVersion: testResult?.serverVersion ?? form.serverVersion ?? "TNP v1 (temniond 0.1.0)",
    };
    onSave(updated);
  };

  return (
    <div className="connection-modal-overlay" onClick={onClose}>
      <div className="connection-modal" onClick={(e) => e.stopPropagation()}>
        <div className="connection-modal-header">
          <div className="connection-modal-title">
            <span>⚙</span>
            <span>Connection Properties — {form.name || "New Connection"}</span>
          </div>
          <button className="tab-close" type="button" onClick={onClose} style={{ fontSize: "16px" }}>
            ✕
          </button>
        </div>

        <div className="connection-modal-tabs">
          <button
            className={`connection-modal-tab-btn ${activeTab === "general" ? "active" : ""}`}
            onClick={() => setActiveTab("general")}
            type="button"
          >
            General
          </button>
          <button
            className={`connection-modal-tab-btn ${activeTab === "network" ? "active" : ""}`}
            onClick={() => setActiveTab("network")}
            type="button"
          >
            Network & Ports
          </button>
          <button
            className={`connection-modal-tab-btn ${activeTab === "ssl" ? "active" : ""}`}
            onClick={() => setActiveTab("ssl")}
            type="button"
          >
            Security & TLS
          </button>
        </div>

        <form onSubmit={handleSubmit}>
          <div className="connection-modal-body">
            {activeTab === "general" && (
              <div className="form-grid">
                <label className="form-group" style={{ gridColumn: "span 2" }}>
                  Connection Name
                  <input
                    className="text-input"
                    value={form.name}
                    onChange={(e) => setForm({ ...form, name: e.target.value })}
                    placeholder="Local Primary Node (TNP)"
                    required
                  />
                </label>
                <label className="form-group">
                  Host / IP Address
                  <input
                    className="text-input"
                    value={form.host}
                    onChange={(e) => setForm({ ...form, host: e.target.value })}
                    placeholder="127.0.0.1"
                    required
                  />
                </label>
                <label className="form-group">
                  Initial Database
                  <input
                    className="text-input"
                    value={form.database}
                    onChange={(e) => setForm({ ...form, database: e.target.value })}
                    placeholder="temnion_default"
                    required
                  />
                </label>
                <label className="form-group">
                  Username
                  <input
                    className="text-input"
                    value={form.username}
                    onChange={(e) => setForm({ ...form, username: e.target.value })}
                    placeholder="temnion_admin"
                    required
                  />
                </label>
                <label className="form-group">
                  Password / Auth Token
                  <div style={{ display: "flex", gap: "6px" }}>
                    <input
                      className="text-input"
                      type={showToken ? "text" : "password"}
                      value={form.authToken ?? ""}
                      onChange={(e) => setForm({ ...form, authToken: e.target.value })}
                      placeholder="••••••••"
                      style={{ flex: 1 }}
                    />
                    <button
                      className="btn btn-secondary btn-sm"
                      type="button"
                      onClick={() => setShowToken(!showToken)}
                    >
                      {showToken ? "Hide" : "Show"}
                    </button>
                  </div>
                </label>
              </div>
            )}

            {activeTab === "network" && (
              <div className="form-grid">
                <label className="form-group">
                  TNP Protocol Port (Binary Framing)
                  <input
                    className="num-input"
                    type="number"
                    value={form.tnpPort}
                    onChange={(e) => setForm({ ...form, tnpPort: Number(e.target.value) })}
                    placeholder="9180"
                    required
                  />
                </label>
                <label className="form-group">
                  Arrow Flight SQL Port (Columnar)
                  <input
                    className="num-input"
                    type="number"
                    value={form.flightPort}
                    onChange={(e) => setForm({ ...form, flightPort: Number(e.target.value) })}
                    placeholder="9181"
                    required
                  />
                </label>
                <div style={{ gridColumn: "span 2", fontSize: "12px", color: "#94a3b8" }}>
                  <div>TNP Default: <code>9180</code> (Low-latency bi-temporal commands & ingestion)</div>
                  <div>Flight SQL Default: <code>9181</code> (Arrow RecordBatch columnar streaming)</div>
                </div>
              </div>
            )}

            {activeTab === "ssl" && (
              <div style={{ display: "flex", flexDirection: "column", gap: "12px" }}>
                <label style={{ display: "flex", alignItems: "center", gap: "10px", cursor: "pointer" }}>
                  <input
                    type="checkbox"
                    checked={form.tls}
                    onChange={(e) => setForm({ ...form, tls: e.target.checked })}
                  />
                  <span style={{ fontSize: "13px", color: "#e2e8f0" }}>Require TLS / SSL Encryption</span>
                </label>
                <div style={{ fontSize: "12px", color: "#94a3b8", lineHeight: 1.6 }}>
                  When TLS is enabled, the connection enforces TLS 1.3 encryption for both TNP and Arrow Flight streams.
                </div>
              </div>
            )}

            {testResult && (
              <div className={`connection-test-result ${testResult.success ? "success" : "error"}`}>
                <span>{testResult.success ? "✓" : "⚠"}</span>
                <div>
                  <div style={{ fontWeight: 600 }}>{testResult.message}</div>
                  {testResult.latencyMs !== undefined && (
                    <div style={{ fontSize: "11px", opacity: 0.9 }}>
                      Round-trip latency: <strong>{testResult.latencyMs} ms</strong> · {testResult.serverVersion}
                    </div>
                  )}
                </div>
              </div>
            )}
          </div>

          <div className="connection-modal-footer">
            <button
              className="btn btn-secondary btn-sm"
              type="button"
              disabled={testing}
              onClick={handleTest}
            >
              {testing ? "Testing Ping..." : "Test Connection"}
            </button>
            <div style={{ display: "flex", gap: "10px" }}>
              <button className="btn btn-secondary btn-sm" type="button" onClick={onClose}>
                Cancel
              </button>
              <button className="btn btn-primary btn-sm" type="submit">
                Save Connection
              </button>
            </div>
          </div>
        </form>
      </div>
    </div>
  );
}
