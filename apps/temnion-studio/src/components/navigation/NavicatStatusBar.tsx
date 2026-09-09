// SPDX-License-Identifier: AGPL-3.0-only

import type { ConnectionProfile } from "../../api";

export interface NavicatStatusBarProps {
  connection: ConnectionProfile;
  eventCount: number;
}

export function NavicatStatusBar({
  connection,
  eventCount,
}: NavicatStatusBarProps) {
  return (
    <footer className="navicat-status-bar">
      <div className="status-bar-left">
        <div className="status-bar-item">
          <span className={`tree-status-dot ${connection.status === "connected" ? "connected" : "disconnected"}`} />
          <span>temnion://{connection.username}@{connection.host}:{connection.tnpPort}/{connection.database}</span>
        </div>
        <div className="status-bar-item">
          <span>Latency: <strong style={{ color: "#34d399" }}>{connection.latencyMs ?? 0.38} ms</strong></span>
        </div>
        <div className="status-bar-item">
          <span>Server: {connection.serverVersion ?? "TNP v1 (temniond 0.1.0)"}</span>
        </div>
      </div>
      <div className="status-bar-right">
        <div className="status-bar-item">
          <span>Events: <strong>{eventCount}</strong></span>
        </div>
        <div className="status-bar-item">
          <span>Database: <code>{connection.database}</code></span>
        </div>
        <div className="status-bar-item">
          <span>Branch: <span style={{ color: "#c084fc" }}>main</span></span>
        </div>
        <div className="status-bar-item" style={{ color: "#10b981", fontWeight: 600 }}>
          <span>#![forbid(unsafe_code)]</span>
        </div>
      </div>
    </footer>
  );
}
