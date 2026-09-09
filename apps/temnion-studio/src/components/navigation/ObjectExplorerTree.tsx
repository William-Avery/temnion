// SPDX-License-Identifier: AGPL-3.0-only

import { useState } from "react";
import type {
  BranchObject,
  ConnectionProfile,
  DatabaseInfo,
  SavedQuery,
  TableDefinition,
  TimeObject,
} from "../../api";

export interface ObjectExplorerTreeProps {
  connections: ConnectionProfile[];
  activeConnId: string;
  databases: DatabaseInfo[];
  tables: TableDefinition[];
  savedQueries: SavedQuery[];
  timeObjects: TimeObject[];
  branches: BranchObject[];
  onSelectConnection: (id: string) => void;
  onEditConnection: (conn: ConnectionProfile) => void;
  onDeleteConnection: (id: string) => void;
  onNewConnection: () => void;
  onSelectDatabase: (name: string, connectionId?: string) => void;
  onNewDatabase: (connectionId?: string) => void;
  onDeleteDatabase: (name: string, connectionId?: string) => void;
  onNewTable: (databaseName?: string) => void;
  onSelectTable: (tableName: string) => void;
  onDeleteTable: (name: string, databaseName?: string) => void;
  onNewQuery: (databaseName?: string) => void;
  onSelectSavedQuery: (query: SavedQuery) => void;
  onDeleteSavedQuery: (id: string) => void;
  onNewTime: (databaseName?: string) => void;
  onSelectTime: (timeObj: TimeObject) => void;
  onDeleteTime: (id: string) => void;
  onNewBranch: (databaseName?: string) => void;
  onSelectBranch: (branch: BranchObject) => void;
  onDeleteBranch: (name: string, databaseName?: string) => void;
  onOpenStorage: () => void;
}

export function ObjectExplorerTree({
  connections,
  activeConnId,
  databases,
  tables,
  savedQueries,
  timeObjects,
  branches,
  onSelectConnection,
  onEditConnection,
  onDeleteConnection,
  onNewConnection,
  onSelectDatabase,
  onNewDatabase,
  onDeleteDatabase,
  onNewTable,
  onSelectTable,
  onDeleteTable,
  onNewQuery,
  onSelectSavedQuery,
  onDeleteSavedQuery,
  onNewTime,
  onSelectTime,
  onDeleteTime,
  onNewBranch,
  onSelectBranch,
  onDeleteBranch,
  onOpenStorage,
}: ObjectExplorerTreeProps) {
  const [expandedConns, setExpandedConns] = useState<Record<string, boolean>>({
    "conn-local-primary": true,
  });
  const [expandedDbs, setExpandedDbs] = useState<Record<string, boolean>>({
    "conn-local-primary:temnion_default": true,
  });
  const [expandedFolders, setExpandedFolders] = useState<Record<string, boolean>>({
    "conn-local-primary:temnion_default-tables": true,
    "conn-local-primary:temnion_default-queries": true,
    "conn-local-primary:temnion_default-time": false,
    "conn-local-primary:temnion_default-branches": false,
    "conn-local-primary:temnion_default-storage": false,
  });

  const toggleConn = (id: string) => {
    setExpandedConns((prev) => ({ ...prev, [id]: !prev[id] }));
  };

  const toggleDb = (dbKey: string) => {
    setExpandedDbs((prev) => ({ ...prev, [dbKey]: !prev[dbKey] }));
  };

  const toggleFolder = (folderKey: string) => {
    setExpandedFolders((prev) => ({ ...prev, [folderKey]: !prev[folderKey] }));
  };

  return (
    <aside className="object-tree-container" aria-label="Database Object Explorer">
      <div className="tree-header">
        <span>Object Explorer</span>
        <div style={{ display: "flex", gap: "6px" }}>
          <button
            className="tree-action-icon-btn"
            title="New Database Connection"
            onClick={onNewConnection}
            type="button"
          >
            +
          </button>
        </div>
      </div>

      <div style={{ padding: "6px 0" }}>
        {connections.map((conn) => {
          const isExpanded = !!expandedConns[conn.id];
          const isActive = conn.id === activeConnId;
          const connDbs = databases.filter((d) => d.connectionId === conn.id);

          return (
            <div className="tree-node-root" key={conn.id}>
              <div
                className={`tree-node-conn ${isActive ? "active" : ""}`}
                onClick={() => toggleConn(conn.id)}
                title={`temnion://${conn.username}@${conn.host}:${conn.tnpPort}/${conn.database}`}
              >
                <div className="tree-conn-left">
                  <span style={{ fontSize: "10px", color: "#94a3b8", width: "12px" }}>
                    {isExpanded ? "▼" : "▶"}
                  </span>
                  <span
                    className={`tree-status-dot ${isActive ? "connected" : "disconnected"}`}
                    title={isActive ? "Connected" : "Disconnected"}
                  />
                  <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", maxWidth: "135px" }}>
                    {conn.name}
                  </span>
                </div>
                <div className="tree-conn-actions" onClick={(e) => e.stopPropagation()}>
                  <button
                    className="tree-action-icon-btn"
                    title="New Database under this connection"
                    onClick={() => onNewDatabase(conn.id)}
                    type="button"
                  >
                    +
                  </button>
                  <button
                    className="tree-action-icon-btn"
                    title="Edit Connection Properties"
                    onClick={() => onEditConnection(conn)}
                    type="button"
                  >
                    ⚙
                  </button>
                  {!isActive && (
                    <button
                      className="tree-action-icon-btn"
                      title="Connect / Make Active"
                      onClick={() => onSelectConnection(conn.id)}
                      type="button"
                    >
                      ⚡
                    </button>
                  )}
                  {conn.id !== "conn-local-primary" && (
                    <button
                      className="tree-action-icon-btn"
                      title="Delete Connection"
                      onClick={() => onDeleteConnection(conn.id)}
                      type="button"
                    >
                      🗑
                    </button>
                  )}
                </div>
              </div>

              {isExpanded && (
                <div className="tree-children">
                  {connDbs.map((db) => {
                    const isDbActive = isActive && conn.database.toLowerCase() === db.name.toLowerCase();
                    const dbKey = `${conn.id}:${db.name}`;
                    const isDbExpanded = expandedDbs[dbKey] ?? (isDbActive || connDbs.length === 1);
                    const dbTables = tables.filter((t) => t.databaseName.toLowerCase() === db.name.toLowerCase());
                    const dbQueries = savedQueries.filter((q) => q.databaseName.toLowerCase() === db.name.toLowerCase());
                    const dbTimes = timeObjects.filter((t) => t.databaseName.toLowerCase() === db.name.toLowerCase());
                    const dbBranches = branches.filter((b) => b.databaseName.toLowerCase() === db.name.toLowerCase());

                    return (
                      <div key={db.id || db.name} style={{ marginBottom: "3px" }}>
                        {/* Database Node */}
                        <div
                          className={`tree-db-folder ${isDbActive ? "active-db" : ""}`}
                          onClick={() => {
                            toggleDb(dbKey);
                            if (!isDbActive) {
                              onSelectDatabase(db.name, conn.id);
                            }
                          }}
                          title={`Database: ${db.name} (${db.clockProfile})`}
                        >
                          <div className="tree-db-left">
                            <span style={{ fontSize: "9px", color: "#94a3b8", width: "10px" }}>
                              {isDbExpanded ? "▼" : "▶"}
                            </span>
                            <span>🗄️</span>
                            <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", maxWidth: "115px" }}>
                              {db.name}
                            </span>
                            {isDbActive && <span className="tree-db-badge">active</span>}
                          </div>
                          <div className="tree-db-actions" onClick={(e) => e.stopPropagation()}>
                            {!db.isDefault && (
                              <button
                                className="tree-item-del-btn"
                                title={`Delete Database ${db.name}`}
                                onClick={() => onDeleteDatabase(db.name, conn.id)}
                                type="button"
                              >
                                🗑
                              </button>
                            )}
                          </div>
                        </div>

                        {isDbExpanded && (
                          <div style={{ paddingLeft: "10px" }}>
                            {/* Tables Folder */}
                            <div>
                              <div
                                className="tree-category-folder"
                                onClick={() => toggleFolder(`${dbKey}-tables`)}
                              >
                                <div className="tree-category-left">
                                  <span style={{ fontSize: "8px" }}>
                                    {expandedFolders[`${dbKey}-tables`] !== false ? "▼" : "▶"}
                                  </span>
                                  <span>📁</span>
                                  <span>Tables</span>
                                  <span className="tree-count-badge">{dbTables.length}</span>
                                </div>
                                <div className="tree-category-actions" onClick={(e) => e.stopPropagation()}>
                                  <button
                                    className="tree-add-btn"
                                    title="New Table"
                                    type="button"
                                    onClick={() => onNewTable(db.name)}
                                  >
                                    +
                                  </button>
                                </div>
                              </div>

                              {expandedFolders[`${dbKey}-tables`] !== false && (
                                <div>
                                  {dbTables.map((t) => (
                                    <div
                                      key={t.id || t.name}
                                      className="tree-leaf-item-container"
                                      onClick={() => onSelectTable(t.name)}
                                      title={`${t.name}: ${t.description}`}
                                    >
                                      <div className="tree-leaf-item-left">
                                        <span style={{ color: "#a5b4fc" }}>⊞</span>
                                        <span>{t.name}</span>
                                      </div>
                                      <div className="tree-leaf-actions" onClick={(e) => e.stopPropagation()}>
                                        <button
                                          className="tree-item-del-btn"
                                          title={`Delete Table ${t.name}`}
                                          type="button"
                                          onClick={() => onDeleteTable(t.name, db.name)}
                                        >
                                          🗑
                                        </button>
                                      </div>
                                    </div>
                                  ))}
                                  {dbTables.length === 0 && (
                                    <div style={{ padding: "3px 0 3px 26px", fontSize: "11px", color: "#64748b", fontStyle: "italic" }}>
                                      No tables yet
                                    </div>
                                  )}
                                </div>
                              )}
                            </div>

                            {/* Queries Folder */}
                            <div>
                              <div
                                className="tree-category-folder"
                                onClick={() => toggleFolder(`${dbKey}-queries`)}
                              >
                                <div className="tree-category-left">
                                  <span style={{ fontSize: "8px" }}>
                                    {expandedFolders[`${dbKey}-queries`] !== false ? "▼" : "▶"}
                                  </span>
                                  <span>📁</span>
                                  <span>Queries</span>
                                  <span className="tree-count-badge">{dbQueries.length}</span>
                                </div>
                                <div className="tree-category-actions" onClick={(e) => e.stopPropagation()}>
                                  <button
                                    className="tree-add-btn"
                                    title="New Query"
                                    type="button"
                                    onClick={() => onNewQuery(db.name)}
                                  >
                                    +
                                  </button>
                                </div>
                              </div>

                              {expandedFolders[`${dbKey}-queries`] !== false && (
                                <div>
                                  {dbQueries.map((q) => (
                                    <div
                                      key={q.id}
                                      className="tree-leaf-item-container"
                                      onClick={() => onSelectSavedQuery(q)}
                                      title={q.name}
                                    >
                                      <div className="tree-leaf-item-left">
                                        <span style={{ color: "#38bdf8" }}>⚡</span>
                                        <span>{q.name}</span>
                                      </div>
                                      <div className="tree-leaf-actions" onClick={(e) => e.stopPropagation()}>
                                        <button
                                          className="tree-item-del-btn"
                                          title={`Delete Query ${q.name}`}
                                          type="button"
                                          onClick={() => onDeleteSavedQuery(q.id)}
                                        >
                                          🗑
                                        </button>
                                      </div>
                                    </div>
                                  ))}
                                  {dbQueries.length === 0 && (
                                    <div style={{ padding: "3px 0 3px 26px", fontSize: "11px", color: "#64748b", fontStyle: "italic" }}>
                                      No saved queries
                                    </div>
                                  )}
                                </div>
                              )}
                            </div>

                            {/* Time Folder */}
                            <div>
                              <div
                                className="tree-category-folder"
                                onClick={() => toggleFolder(`${dbKey}-time`)}
                              >
                                <div className="tree-category-left">
                                  <span style={{ fontSize: "8px" }}>
                                    {expandedFolders[`${dbKey}-time`] ? "▼" : "▶"}
                                  </span>
                                  <span>📁</span>
                                  <span>Time</span>
                                  <span className="tree-count-badge">{dbTimes.length}</span>
                                </div>
                                <div className="tree-category-actions" onClick={(e) => e.stopPropagation()}>
                                  <button
                                    className="tree-add-btn"
                                    title="New Time Horizon / Checkpoint"
                                    type="button"
                                    onClick={() => onNewTime(db.name)}
                                  >
                                    +
                                  </button>
                                </div>
                              </div>

                              {expandedFolders[`${dbKey}-time`] && (
                                <div>
                                  {dbTimes.map((t) => (
                                    <div
                                      key={t.id}
                                      className="tree-leaf-item-container"
                                      onClick={() => onSelectTime(t)}
                                      title={`${t.name} (${t.resolution}): ${t.description}`}
                                    >
                                      <div className="tree-leaf-item-left">
                                        <span style={{ color: "#34d399" }}>◴</span>
                                        <span>{t.name}</span>
                                      </div>
                                      <div className="tree-leaf-actions" onClick={(e) => e.stopPropagation()}>
                                        <button
                                          className="tree-item-del-btn"
                                          title={`Delete Time Object ${t.name}`}
                                          type="button"
                                          onClick={() => onDeleteTime(t.id)}
                                        >
                                          🗑
                                        </button>
                                      </div>
                                    </div>
                                  ))}
                                  {dbTimes.length === 0 && (
                                    <div style={{ padding: "3px 0 3px 26px", fontSize: "11px", color: "#64748b", fontStyle: "italic" }}>
                                      No time horizons
                                    </div>
                                  )}
                                </div>
                              )}
                            </div>

                            {/* Branches Folder */}
                            <div>
                              <div
                                className="tree-category-folder"
                                onClick={() => toggleFolder(`${dbKey}-branches`)}
                              >
                                <div className="tree-category-left">
                                  <span style={{ fontSize: "8px" }}>
                                    {expandedFolders[`${dbKey}-branches`] ? "▼" : "▶"}
                                  </span>
                                  <span>📁</span>
                                  <span>Branches</span>
                                  <span className="tree-count-badge">{dbBranches.length}</span>
                                </div>
                                <div className="tree-category-actions" onClick={(e) => e.stopPropagation()}>
                                  <button
                                    className="tree-add-btn"
                                    title="New Branch"
                                    type="button"
                                    onClick={() => onNewBranch(db.name)}
                                  >
                                    +
                                  </button>
                                </div>
                              </div>

                              {expandedFolders[`${dbKey}-branches`] && (
                                <div>
                                  {dbBranches.map((b) => (
                                    <div
                                      key={b.id || b.name}
                                      className="tree-leaf-item-container"
                                      onClick={() => onSelectBranch(b)}
                                      title={`Branch: ${b.name} (${b.lifecycle})`}
                                    >
                                      <div className="tree-leaf-item-left">
                                        {b.name === "main" ? (
                                          <span style={{ color: "#34d399" }}>●</span>
                                        ) : (
                                          <span style={{ color: "#c084fc" }}>⑂</span>
                                        )}
                                        <span style={b.name === "main" ? { color: "#a7f3d0", fontWeight: 600 } : {}}>
                                          {b.name} {b.name === "main" ? "(active)" : ""}
                                        </span>
                                      </div>
                                      <div className="tree-leaf-actions" onClick={(e) => e.stopPropagation()}>
                                        {b.name !== "main" && (
                                          <button
                                            className="tree-item-del-btn"
                                            title={`Delete Branch ${b.name}`}
                                            type="button"
                                            onClick={() => onDeleteBranch(b.name, db.name)}
                                          >
                                            🗑
                                          </button>
                                        )}
                                      </div>
                                    </div>
                                  ))}
                                </div>
                              )}
                            </div>

                            {/* Storage & Engine Folder */}
                            <div>
                              <div
                                className="tree-category-folder"
                                onClick={() => toggleFolder(`${dbKey}-storage`)}
                              >
                                <div className="tree-category-left">
                                  <span style={{ fontSize: "8px" }}>
                                    {expandedFolders[`${dbKey}-storage`] ? "▼" : "▶"}
                                  </span>
                                  <span>📁</span>
                                  <span>Storage & Engine</span>
                                </div>
                              </div>

                              {expandedFolders[`${dbKey}-storage`] && (
                                <div>
                                  <div className="tree-leaf-item-container" onClick={onOpenStorage}>
                                    <div className="tree-leaf-item-left">
                                      <span>📄</span>
                                      <span>Active WAL Prefix</span>
                                    </div>
                                  </div>
                                  <div className="tree-leaf-item-container" onClick={onOpenStorage}>
                                    <div className="tree-leaf-item-left">
                                      <span>📄</span>
                                      <span>SegmentManifest</span>
                                    </div>
                                  </div>
                                  <div className="tree-leaf-item-container" onClick={onOpenStorage}>
                                    <div className="tree-leaf-item-left">
                                      <span>📄</span>
                                      <span>TSF Segments</span>
                                    </div>
                                  </div>
                                </div>
                              )}
                            </div>
                          </div>
                        )}
                      </div>
                    );
                  })}
                  {connDbs.length === 0 && (
                    <div style={{ padding: "6px 14px", fontSize: "11px", color: "#64748b", fontStyle: "italic" }}>
                      No databases registered. Click + to create one.
                    </div>
                  )}
                </div>
              )}
            </div>
          );
        })}
      </div>
    </aside>
  );
}
