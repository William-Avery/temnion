// SPDX-License-Identifier: AGPL-3.0-only

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import {
  type ConnectionProfile,
  type EngineStatus,
  connectDatabase,
  createDatabase,
  deleteConnection,
  disconnectDatabase,
  isNativeRuntime,
  listConnections,
  saveConnection,
  setActiveConnection,
  testConnection,
} from "../../api";
import { errorText } from "../../utils/formatters";
import { Notice, PanelHeader } from "../common";

export interface ConnectionsPanelProps {
  status: EngineStatus;
  onEditConnection?: (conn: ConnectionProfile) => void;
}

export function ConnectionsPanel({
  status,
  onEditConnection,
}: ConnectionsPanelProps) {
  const queryClient = useQueryClient();
  const connectionsQuery = useQuery({ queryKey: ["connections"], queryFn: listConnections });
  const connections = connectionsQuery.data ?? [];

  const [activeConnId, setActiveConnId] = useState("conn-local-primary");
  const [showAddForm, setShowAddForm] = useState(false);
  const [testStatus, setTestStatus] = useState<{ success: boolean; message: string; latencyMs: number } | null>(null);

  const [form, setForm] = useState<{
    id?: string;
    name: string;
    host: string;
    tnpPort: number;
    flightPort: number;
    database: string;
    username: string;
    authToken: string;
    tls: boolean;
  }>({
    id: undefined,
    name: "New Connection",
    host: "127.0.0.1",
    tnpPort: 9180,
    flightPort: 9181,
    database: "temnion_default",
    username: "temnion_admin",
    authToken: "",
    tls: false,
  });

  const [embeddedPath, setEmbeddedPath] = useState(status.path ?? "");

  const saveMutation = useMutation({
    mutationFn: (profile: ConnectionProfile) => saveConnection(profile),
    onSuccess: (updated) => {
      queryClient.setQueryData(["connections"], updated);
      setShowAddForm(false);
      setTestStatus(null);
    },
  });

  const deleteMutation = useMutation({
    mutationFn: (id: string) => deleteConnection(id),
    onSuccess: (updated) => queryClient.setQueryData(["connections"], updated),
  });

  const testMutation = useMutation({
    mutationFn: (profile: Partial<ConnectionProfile>) => testConnection(profile),
    onSuccess: (res) => {
      setTestStatus({
        success: res.success,
        message: `${res.message} (Round-trip: ${res.latencyMs} ms)`,
        latencyMs: res.latencyMs,
      });
    },
    onError: (err) => {
      setTestStatus({
        success: false,
        message: errorText(err),
        latencyMs: 0,
      });
    },
  });

  const switchMutation = useMutation({
    mutationFn: (id: string) => setActiveConnection(id),
    onSuccess: (active) => {
      setActiveConnId(active.id);
      queryClient.invalidateQueries({ queryKey: ["engine-status"] });
    },
  });

  const connectEmbedded = useMutation({
    mutationFn: (mode: "open" | "create") => mode === "open" ? connectDatabase(embeddedPath) : createDatabase(embeddedPath),
    onSuccess: (next) => queryClient.setQueryData(["engine-status"], next),
  });

  const disconnectEmbedded = useMutation({
    mutationFn: disconnectDatabase,
    onSuccess: (next) => {
      queryClient.setQueryData(["engine-status"], next);
      queryClient.removeQueries({ queryKey: ["history"] });
      queryClient.removeQueries({ queryKey: ["branches"] });
    },
  });

  const handleEditInline = (conn: ConnectionProfile) => {
    setForm({
      id: conn.id,
      name: conn.name,
      host: conn.host,
      tnpPort: conn.tnpPort,
      flightPort: conn.flightPort,
      database: conn.database,
      username: conn.username,
      authToken: conn.authToken ?? "",
      tls: conn.tls,
    });
    setShowAddForm(true);
  };

  const handleSave = () => {
    const profile: ConnectionProfile = {
      id: form.id || `conn-${Date.now()}`,
      name: form.name,
      host: form.host,
      tnpPort: form.tnpPort,
      flightPort: form.flightPort,
      database: form.database,
      username: form.username,
      authToken: form.authToken ? "••••••••" : undefined,
      tls: form.tls,
      lastConnected: "Just now",
      status: "connected",
      latencyMs: testStatus?.latencyMs ?? 0.38,
      serverVersion: "TNP v1 (temniond 0.1.0)",
    };
    saveMutation.mutate(profile);
  };

  return (
    <section className="view-panel active">
      <PanelHeader
        title="Connections Manager"
        subtitle="Manage database connections, configure network ports, superuser credentials, and test server latency."
      />

      {/* Top Action Bar */}
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "16px" }}>
        <div style={{ fontSize: "14px", color: "#94a3b8" }}>
          Configured server endpoints recognized by Temnion Studio Workbench, CLI, and external consumers.
        </div>
        <button
          className="btn btn-primary btn-sm"
          type="button"
          onClick={() => {
            if (onEditConnection) {
              onEditConnection({
                id: "",
                name: "New Connection",
                host: "127.0.0.1",
                tnpPort: 9180,
                flightPort: 9181,
                database: "temnion_default",
                username: "temnion_admin",
                authToken: "",
                tls: false,
                status: "disconnected",
                latencyMs: 0.38,
                serverVersion: "TNP v1 (temniond 0.1.0)",
              });
            } else {
              setShowAddForm(!showAddForm);
            }
          }}
        >
          {showAddForm ? "Cancel" : "+ Add New Connection"}
        </button>
      </div>

      {/* Test Status Alert */}
      {testStatus && (
        <Notice kind={testStatus.success ? "success" : "error"}>
          {testStatus.message}
        </Notice>
      )}

      {/* Add / Edit Connection Form */}
      {showAddForm && (
        <div className="glass-card" style={{ padding: "16px", marginBottom: "20px", border: "1px solid #38bdf8" }}>
          <div style={{ fontSize: "15px", fontWeight: "bold", color: "#e2e8f0", marginBottom: "12px" }}>
            {form.id ? `Edit Connection: ${form.name}` : "New Temnion Database Connection"}
          </div>
          <div className="form-grid" style={{ marginBottom: "12px" }}>
            <label className="form-group">
              Connection Name
              <input
                className="text-input"
                value={form.name}
                onChange={(e) => setForm({ ...form, name: e.target.value })}
                placeholder="e.g. Production Primary"
              />
            </label>
            <label className="form-group">
              Host / Address
              <input
                className="text-input"
                value={form.host}
                onChange={(e) => setForm({ ...form, host: e.target.value })}
                placeholder="127.0.0.1"
              />
            </label>
            <label className="form-group">
              TNP Network Port
              <input
                className="num-input"
                type="number"
                value={form.tnpPort}
                onChange={(e) => setForm({ ...form, tnpPort: Number(e.target.value) })}
                placeholder="9180"
              />
            </label>
            <label className="form-group">
              Arrow Flight Port
              <input
                className="num-input"
                type="number"
                value={form.flightPort}
                onChange={(e) => setForm({ ...form, flightPort: Number(e.target.value) })}
                placeholder="9181"
              />
            </label>
            <label className="form-group">
              Database Name
              <input
                className="text-input"
                value={form.database}
                onChange={(e) => setForm({ ...form, database: e.target.value })}
                placeholder="temnion_default"
              />
            </label>
            <label className="form-group">
              Username
              <input
                className="text-input"
                value={form.username}
                onChange={(e) => setForm({ ...form, username: e.target.value })}
                placeholder="temnion_admin"
              />
            </label>
            <label className="form-group">
              Password / Auth Token
              <input
                className="text-input"
                type="password"
                value={form.authToken}
                onChange={(e) => setForm({ ...form, authToken: e.target.value })}
                placeholder="••••••••••••"
              />
            </label>
            <label className="form-group" style={{ display: "flex", alignItems: "center", gap: "8px", marginTop: "24px" }}>
              <input
                type="checkbox"
                checked={form.tls}
                onChange={(e) => setForm({ ...form, tls: e.target.checked })}
              />
              <span style={{ fontSize: "12px", color: "#e2e8f0" }}>Enable TLS / SSL</span>
            </label>
          </div>

          <div style={{ display: "flex", gap: "10px", justifyContent: "flex-end" }}>
            <button
              className="btn btn-secondary btn-sm"
              type="button"
              disabled={testMutation.isPending}
              onClick={() => testMutation.mutate(form)}
            >
              {testMutation.isPending ? "Testing..." : "Test Connection"}
            </button>
            <button
              className="btn btn-primary btn-sm"
              type="button"
              onClick={handleSave}
            >
              Save Connection
            </button>
          </div>
        </div>
      )}

      {/* Saved Connections Grid */}
      <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(320px, 1fr))", gap: "14px", marginBottom: "24px" }}>
        {connections.map((conn) => {
          const isActive = conn.id === activeConnId;
          return (
            <div
              key={conn.id}
              className={`glass-card conn-card ${isActive ? "active-conn" : ""}`}
              style={{ padding: "16px" }}
            >
              <div className="conn-header" style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "8px" }}>
                <div style={{ fontWeight: "bold", fontSize: "15px", color: "#e2e8f0" }}>{conn.name}</div>
                <span className={`badge ${isActive ? "badge-success" : "badge-neutral"}`}>
                  {isActive ? "Active (Connected)" : conn.status}
                </span>
              </div>

              <div style={{ fontSize: "12px", fontFamily: "monospace", color: "#38bdf8", marginBottom: "10px" }}>
                temnion://{conn.username}@{conn.host}:{conn.tnpPort}/{conn.database}
              </div>

              <div style={{ display: "grid", gridTemplateColumns: "repeat(2, 1fr)", gap: "6px", fontSize: "11px", color: "#94a3b8", marginBottom: "14px" }}>
                <div>TNP Port: <span style={{ color: "#e2e8f0" }}>{conn.tnpPort}</span></div>
                <div>Flight Port: <span style={{ color: "#e2e8f0" }}>{conn.flightPort}</span></div>
                <div>Protocol: <span style={{ color: "#e2e8f0" }}>{conn.serverVersion ?? "TNP v1"}</span></div>
                <div>Latency: <span style={{ color: "#34d399" }}>{conn.latencyMs ? `${conn.latencyMs} ms` : "—"}</span></div>
              </div>

              <div style={{ display: "flex", gap: "8px", justifyContent: "flex-end", flexWrap: "wrap" }}>
                <button
                  className="btn btn-secondary btn-sm"
                  type="button"
                  onClick={() => testMutation.mutate(conn)}
                >
                  Test
                </button>
                <button
                  className="btn btn-secondary btn-sm"
                  type="button"
                  onClick={() => onEditConnection ? onEditConnection(conn) : handleEditInline(conn)}
                >
                  ⚙ Edit Properties
                </button>
                {conn.id !== "conn-local-primary" && (
                  <button
                    className="btn btn-secondary btn-sm"
                    type="button"
                    onClick={() => deleteMutation.mutate(conn.id)}
                  >
                    Delete
                  </button>
                )}
                <button
                  className={`btn ${isActive ? "btn-secondary" : "btn-primary"} btn-sm`}
                  type="button"
                  disabled={isActive || switchMutation.isPending}
                  onClick={() => switchMutation.mutate(conn.id)}
                >
                  {isActive ? "Active" : "Connect"}
                </button>
              </div>
            </div>
          );
        })}
      </div>

      {/* Embedded Directory Store Fallback */}
      <div className="glass-card" style={{ padding: "16px" }}>
        <div style={{ fontSize: "14px", fontWeight: "bold", color: "#e2e8f0", marginBottom: "6px" }}>
          Local Embedded Store (Direct Filesystem)
        </div>
        <div style={{ fontSize: "12px", color: "#94a3b8", marginBottom: "12px" }}>
          Open a local database directory directly in embedded mode without running a server daemon.
        </div>
        <label className="form-group">
          Database Directory Path
          <input
            className="text-input path-input"
            placeholder="C:\ProgramData\Temnion\data or ./data/temnion_db"
            value={embeddedPath}
            onChange={(e) => setEmbeddedPath(e.target.value)}
          />
        </label>
        <div style={{ display: "flex", gap: "8px", marginTop: "10px" }}>
          <button
            className="btn btn-primary btn-sm"
            disabled={connectEmbedded.isPending || !isNativeRuntime}
            onClick={() => connectEmbedded.mutate("open")}
            type="button"
          >
            Open Existing Store
          </button>
          <button
            className="btn btn-secondary btn-sm"
            disabled={connectEmbedded.isPending || !isNativeRuntime}
            onClick={() => connectEmbedded.mutate("create")}
            type="button"
          >
            Create New Store
          </button>
          <button
            className="btn btn-secondary btn-sm"
            disabled={!status.connected || disconnectEmbedded.isPending}
            onClick={() => disconnectEmbedded.mutate()}
            type="button"
          >
            Disconnect
          </button>
        </div>
      </div>
    </section>
  );
}
