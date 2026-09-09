// SPDX-License-Identifier: AGPL-3.0-only

import React, { useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import {
  type ConnectionProfile,
  type CreateDatabaseOptions,
  type NewBranchOptions,
  type NewQueryOptions,
  type NewTableOptions,
  type NewTimeOptions,
  browserStatus,
  createBranchObject,
  createDatabaseCatalog,
  createTable,
  createTimeObject,
  deleteBranchObject,
  deleteConnection,
  deleteDatabaseCatalog,
  deleteSavedQuery,
  deleteTable,
  deleteTimeObject,
  getEngineStatus,
  listBranchObjects,
  listConnections,
  listDatabases,
  listSavedQueries,
  listTables,
  listTimeObjects,
  saveConnection,
  saveQuery,
  setActiveConnection,
  switchActiveDatabase,
} from "./api";

import type { StudioTab, View } from "./types";
import { navItems } from "./types";

import { Logo } from "./components/common";
import {
  NavicatRibbon,
  NavicatStatusBar,
  NavicatTabsBar,
  ObjectExplorerTree,
} from "./components/navigation";
// Lazy-loaded panel components for code-splitting and rapid initial bundle load
const CausalityPanel = React.lazy(() =>
  import("./components/panels/CausalityPanel").then((m) => ({ default: m.CausalityPanel }))
);
const ConnectionsPanel = React.lazy(() =>
  import("./components/panels/ConnectionsPanel").then((m) => ({ default: m.ConnectionsPanel }))
);
const HistoryPanel = React.lazy(() =>
  import("./components/panels/HistoryPanel").then((m) => ({ default: m.HistoryPanel }))
);
const IngestPanel = React.lazy(() =>
  import("./components/panels/IngestPanel").then((m) => ({ default: m.IngestPanel }))
);
const MetricsPanel = React.lazy(() =>
  import("./components/panels/MetricsPanel").then((m) => ({ default: m.MetricsPanel }))
);
const QueryPanel = React.lazy(() =>
  import("./components/panels/QueryPanel").then((m) => ({ default: m.QueryPanel }))
);
const SchemaPanel = React.lazy(() =>
  import("./components/panels/SchemaPanel").then((m) => ({ default: m.SchemaPanel }))
);
const TableViewerPanel = React.lazy(() =>
  import("./components/panels/TableViewerPanel").then((m) => ({ default: m.TableViewerPanel }))
);
const GuidePanel = React.lazy(() =>
  import("./Guide").then((m) => ({ default: m.GuidePanel }))
);

// Lazy-loaded modal components
const ConnectionPropertiesModal = React.lazy(() =>
  import("./components/modals/ConnectionPropertiesModal").then((m) => ({ default: m.ConnectionPropertiesModal }))
);
const CreateBranchModal = React.lazy(() =>
  import("./components/modals/CreateBranchModal").then((m) => ({ default: m.CreateBranchModal }))
);
const CreateDatabaseModal = React.lazy(() =>
  import("./components/modals/CreateDatabaseModal").then((m) => ({ default: m.CreateDatabaseModal }))
);
const CreateQueryModal = React.lazy(() =>
  import("./components/modals/CreateQueryModal").then((m) => ({ default: m.CreateQueryModal }))
);
const CreateTableModal = React.lazy(() =>
  import("./components/modals/CreateTableModal").then((m) => ({ default: m.CreateTableModal }))
);
const CreateTimeModal = React.lazy(() =>
  import("./components/modals/CreateTimeModal").then((m) => ({ default: m.CreateTimeModal }))
);

function StudioLoadingFallback() {
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        height: "100%",
        minHeight: "240px",
        color: "var(--text-muted, #94a3b8)",
        gap: "0.75rem",
        fontSize: "0.9rem",
      }}
    >
      <span className="pulse-dot" />
      <span>Loading workspace view...</span>
    </div>
  );
}


export function App() {
  const queryClient = useQueryClient();
  const statusQuery = useQuery({ queryKey: ["engine-status"], queryFn: getEngineStatus, initialData: browserStatus });
  const status = statusQuery.data;

  const connectionsQuery = useQuery({ queryKey: ["connections"], queryFn: listConnections });
  const connections = connectionsQuery.data ?? [
    {
      id: "conn-local-primary",
      name: "Local Primary Node (TNP)",
      host: "127.0.0.1",
      tnpPort: 9180,
      flightPort: 9181,
      database: "temnion_default",
      username: "temnion_admin",
      authToken: "••••••••",
      tls: false,
      lastConnected: "Just now",
      status: "connected" as const,
      latencyMs: 0.38,
      serverVersion: "TNP v1 (temniond 0.1.0)",
    },
  ];

  const [activeConnId, setActiveConnId] = useState<string>("conn-local-primary");
  const activeConnection = useMemo(() => {
    return connections.find((c) => c.id === activeConnId) ?? connections[0];
  }, [connections, activeConnId]);

  const [editingConnection, setEditingConnection] = useState<ConnectionProfile | null>(null);

  // Database Catalog and Objects queries
  const databasesQuery = useQuery({
    queryKey: ["databases"],
    queryFn: () => listDatabases(),
  });
  const databases = databasesQuery.data ?? [
    {
      id: "db-local-default",
      name: "temnion_default",
      connectionId: "conn-local-primary",
      clockProfile: "Canonical Clock #1 (Physical UTC + Lamport + Causal DAG)",
      template: "Industrial IoT & Telemetry",
      storageTarget: "managed" as const,
      tablesCount: 4,
      queriesCount: 3,
      timeObjectsCount: 3,
      branchesCount: 3,
      eventCount: 28,
      isDefault: true,
      createdAt: "System Bootstrap",
    },
  ];

  const tablesQuery = useQuery({
    queryKey: ["tables", activeConnection.database],
    queryFn: () => listTables(activeConnection.database),
  });
  const tablesData = tablesQuery.data ?? [];

  const savedQueriesQuery = useQuery({
    queryKey: ["saved-queries", activeConnection.database],
    queryFn: () => listSavedQueries(activeConnection.database),
  });
  const savedQueriesData = savedQueriesQuery.data ?? [];

  const timeObjectsQuery = useQuery({
    queryKey: ["time-objects", activeConnection.database],
    queryFn: () => listTimeObjects(activeConnection.database),
  });
  const timeObjectsData = timeObjectsQuery.data ?? [];

  const branchesQuery = useQuery({
    queryKey: ["branch-objects", activeConnection.database],
    queryFn: () => listBranchObjects(activeConnection.database),
  });
  const branchObjectsData = branchesQuery.data ?? [];

  // Modal dialog states
  const [createDbOpen, setCreateDbOpen] = useState(false);
  const [createDbInitialConnId, setCreateDbInitialConnId] = useState<string>("conn-local-primary");
  const [createTableOpen, setCreateTableOpen] = useState(false);
  const [createQueryOpen, setCreateQueryOpen] = useState(false);
  const [createTimeOpen, setCreateTimeOpen] = useState(false);
  const [createBranchOpen, setCreateBranchOpen] = useState(false);

  const [tabs, setTabs] = useState<StudioTab[]>([
    { id: "tab-guide", type: "guide", title: "How-To Guide", icon: "📖" },
    { id: "tab-query-1", type: "query", title: "Query 1", icon: "⚡" },
  ]);
  const [activeTabId, setActiveTabId] = useState<string>("tab-query-1");
  const [queryCount, setQueryCount] = useState<number>(1);

  // Connection mutations
  const switchMutation = useMutation({
    mutationFn: (id: string) => setActiveConnection(id),
    onSuccess: (active) => {
      setActiveConnId(active.id);
      queryClient.invalidateQueries({ queryKey: ["engine-status"] });
      queryClient.invalidateQueries({ queryKey: ["databases"] });
      queryClient.invalidateQueries({ queryKey: ["tables"] });
      queryClient.invalidateQueries({ queryKey: ["saved-queries"] });
      queryClient.invalidateQueries({ queryKey: ["time-objects"] });
      queryClient.invalidateQueries({ queryKey: ["branch-objects"] });
    },
  });

  const saveMutation = useMutation({
    mutationFn: (profile: ConnectionProfile) => saveConnection(profile),
    onSuccess: (updated) => {
      queryClient.setQueryData(["connections"], updated);
      setEditingConnection(null);
    },
  });

  const deleteMutation = useMutation({
    mutationFn: (id: string) => deleteConnection(id),
    onSuccess: (updated) => queryClient.setQueryData(["connections"], updated),
  });

  // Database mutations
  const createDbMutation = useMutation({
    mutationFn: (opts: CreateDatabaseOptions) => createDatabaseCatalog(opts),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["databases"] });
      queryClient.invalidateQueries({ queryKey: ["connections"] });
      queryClient.invalidateQueries({ queryKey: ["tables"] });
      queryClient.invalidateQueries({ queryKey: ["saved-queries"] });
      queryClient.invalidateQueries({ queryKey: ["time-objects"] });
      queryClient.invalidateQueries({ queryKey: ["branch-objects"] });
      setCreateDbOpen(false);
    },
  });

  const deleteDbMutation = useMutation({
    mutationFn: ({ name, connId }: { name: string; connId?: string }) =>
      deleteDatabaseCatalog(name, connId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["databases"] });
      queryClient.invalidateQueries({ queryKey: ["connections"] });
      queryClient.invalidateQueries({ queryKey: ["tables"] });
      queryClient.invalidateQueries({ queryKey: ["saved-queries"] });
      queryClient.invalidateQueries({ queryKey: ["time-objects"] });
      queryClient.invalidateQueries({ queryKey: ["branch-objects"] });
    },
  });

  const switchDbMutation = useMutation({
    mutationFn: ({ name, connId }: { name: string; connId?: string }) =>
      switchActiveDatabase(name, connId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["databases"] });
      queryClient.invalidateQueries({ queryKey: ["connections"] });
      queryClient.invalidateQueries({ queryKey: ["tables"] });
      queryClient.invalidateQueries({ queryKey: ["saved-queries"] });
      queryClient.invalidateQueries({ queryKey: ["time-objects"] });
      queryClient.invalidateQueries({ queryKey: ["branch-objects"] });
    },
  });

  // Database objects mutations
  const createTableMutation = useMutation({
    mutationFn: (opts: NewTableOptions) => createTable(opts),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["tables"] });
      queryClient.invalidateQueries({ queryKey: ["databases"] });
      setCreateTableOpen(false);
    },
  });

  const deleteTableMutation = useMutation({
    mutationFn: ({ name, dbName }: { name: string; dbName?: string }) =>
      deleteTable(name, dbName),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["tables"] });
      queryClient.invalidateQueries({ queryKey: ["databases"] });
    },
  });

  const createQueryMutation = useMutation({
    mutationFn: (opts: NewQueryOptions) => saveQuery(opts),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["saved-queries"] });
      queryClient.invalidateQueries({ queryKey: ["databases"] });
      setCreateQueryOpen(false);
    },
  });

  const deleteQueryMutation = useMutation({
    mutationFn: (id: string) => deleteSavedQuery(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["saved-queries"] });
      queryClient.invalidateQueries({ queryKey: ["databases"] });
    },
  });

  const createTimeMutation = useMutation({
    mutationFn: (opts: NewTimeOptions) => createTimeObject(opts),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["time-objects"] });
      queryClient.invalidateQueries({ queryKey: ["databases"] });
      setCreateTimeOpen(false);
    },
  });

  const deleteTimeMutation = useMutation({
    mutationFn: (id: string) => deleteTimeObject(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["time-objects"] });
      queryClient.invalidateQueries({ queryKey: ["databases"] });
    },
  });

  const createBranchMutation = useMutation({
    mutationFn: (opts: NewBranchOptions) => createBranchObject(opts),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["branch-objects"] });
      queryClient.invalidateQueries({ queryKey: ["databases"] });
      setCreateBranchOpen(false);
    },
  });

  const deleteBranchMutation = useMutation({
    mutationFn: ({ name, dbName }: { name: string; dbName?: string }) =>
      deleteBranchObject(name, dbName),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["branch-objects"] });
      queryClient.invalidateQueries({ queryKey: ["databases"] });
    },
  });

  const openTab = (
    type: View | "table",
    title: string,
    icon: string,
    extra?: { tableName?: string; querySnippet?: string }
  ) => {
    const existing = tabs.find((t) => {
      if (type === "table") return t.type === "table" && t.tableName === extra?.tableName;
      if (type === "query") return false;
      return t.type === type;
    });

    if (existing) {
      setActiveTabId(existing.id);
    } else {
      const newId = `tab-${type}-${Date.now()}`;
      setTabs((prev) => [
        ...prev,
        {
          id: newId,
          type,
          title,
          icon,
          tableName: extra?.tableName,
          querySnippet: extra?.querySnippet,
        },
      ]);
      setActiveTabId(newId);
    }
  };

  const handleNewQueryTab = (snippet?: string) => {
    const nextCount = queryCount + 1;
    setQueryCount(nextCount);
    const newId = `tab-query-${nextCount}`;
    setTabs((prev) => [
      ...prev,
      {
        id: newId,
        type: "query",
        title: `Query ${nextCount}`,
        icon: "⚡",
        querySnippet: snippet,
      },
    ]);
    setActiveTabId(newId);
  };

  const handleCloseTab = (idToClose: string) => {
    if (tabs.length <= 1) return;
    const nextTabs = tabs.filter((t) => t.id !== idToClose);
    setTabs(nextTabs);
    if (activeTabId === idToClose) {
      setActiveTabId(nextTabs[nextTabs.length - 1].id);
    }
  };

  const activeTab = tabs.find((t) => t.id === activeTabId) ?? tabs[0];

  const handleNavigate = (targetView: View, snippet?: string) => {
    if (targetView === "query") {
      handleNewQueryTab(snippet);
    } else {
      const nav = navItems.find((item) => item.id === targetView);
      openTab(targetView, nav ? nav.label : targetView, nav ? nav.icon : "•", { querySnippet: snippet });
    }
  };

  let tabContent: React.ReactNode;
  switch (activeTab.type) {
    case "guide":
      tabContent = <GuidePanel onNavigate={handleNavigate} />;
      break;
    case "query":
      tabContent = (
        <QueryPanel
          connected={status.connected}
          initialQuery={activeTab.querySnippet}
          onNavigateToConnections={() => openTab("connections", "Connections", "◎")}
          onNavigateToGuide={() => openTab("guide", "How-To Guide", "📖")}
        />
      );
      break;
    case "table":
      tabContent = (
        <TableViewerPanel
          tableName={activeTab.tableName ?? "SensorTelemetry"}
          onRunQuery={(q) => handleNewQueryTab(q)}
        />
      );
      break;
    case "history":
      tabContent = <HistoryPanel connected={status.connected} eventCount={status.eventCount} />;
      break;
    case "causality":
      tabContent = <CausalityPanel connected={status.connected} eventCount={status.eventCount} />;
      break;
    case "schemas":
      tabContent = <SchemaPanel />;
      break;
    case "ingest":
      tabContent = <IngestPanel connected={status.connected} />;
      break;
    case "connections":
      tabContent = (
        <ConnectionsPanel
          status={status}
          onEditConnection={(conn) => setEditingConnection(conn)}
        />
      );
      break;
    case "metrics":
      tabContent = <MetricsPanel status={status} />;
      break;
  }

  return (
    <div className="studio-layout">
      <header className="studio-header">
        <Logo />
        <NavicatRibbon
          onNewConnection={() =>
            setEditingConnection({
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
            })
          }
          onEditActiveConnection={() => setEditingConnection(activeConnection)}
          onNewDatabase={() => {
            setCreateDbInitialConnId(activeConnId);
            setCreateDbOpen(true);
          }}
          onNewTable={() => setCreateTableOpen(true)}
          onNewQuery={() => setCreateQueryOpen(true)}
          onNewTime={() => setCreateTimeOpen(true)}
          onNewBranch={() => setCreateBranchOpen(true)}
          onOpenIngest={() => openTab("ingest", "Ingestion", "⇧")}
          onOpenStorage={() => openTab("metrics", "Storage & Health", "▥")}
          onOpenGuide={() => openTab("guide", "How-To Guide", "📖")}
          onRefresh={() => {
            queryClient.invalidateQueries();
          }}
        />
        <div className="header-status">
          <div
            className={`status-chip ${status.connected ? "connected" : ""}`}
            onClick={() => openTab("connections", "Connections", "◎")}
            style={{ cursor: "pointer" }}
            title="Click to manage database connections"
          >
            <span className="pulse-dot" />
            <span>{status.connected ? `Connected · ${status.eventCount} events` : "No database connected"}</span>
          </div>
          <div className="status-chip branch-chip">main timeline</div>
          <div className="status-chip mode-chip">TNP Port {activeConnection.tnpPort}</div>
        </div>
      </header>

      <div className="studio-body">
        <ObjectExplorerTree
          connections={connections}
          activeConnId={activeConnId}
          databases={databases}
          tables={tablesData}
          savedQueries={savedQueriesData}
          timeObjects={timeObjectsData}
          branches={branchObjectsData}
          onSelectConnection={(id) => switchMutation.mutate(id)}
          onEditConnection={(conn) => setEditingConnection(conn)}
          onDeleteConnection={(id) => deleteMutation.mutate(id)}
          onNewConnection={() =>
            setEditingConnection({
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
            })
          }
          onSelectDatabase={(name, connId) => switchDbMutation.mutate({ name, connId })}
          onNewDatabase={(connId) => {
            setCreateDbInitialConnId(connId || activeConnId);
            setCreateDbOpen(true);
          }}
          onDeleteDatabase={(name, connId) => deleteDbMutation.mutate({ name, connId })}
          onNewTable={(dbName) => {
            if (dbName && dbName.toLowerCase() !== activeConnection.database.toLowerCase()) {
              switchDbMutation.mutate({ name: dbName });
            }
            setCreateTableOpen(true);
          }}
          onSelectTable={(tableName) => openTab("table", `Table: ${tableName}`, "⊞", { tableName })}
          onDeleteTable={(name, dbName) => deleteTableMutation.mutate({ name, dbName })}
          onNewQuery={(dbName) => {
            if (dbName && dbName.toLowerCase() !== activeConnection.database.toLowerCase()) {
              switchDbMutation.mutate({ name: dbName });
            }
            setCreateQueryOpen(true);
          }}
          onSelectSavedQuery={(q) => handleNewQueryTab(q.queryText)}
          onDeleteSavedQuery={(id) => deleteQueryMutation.mutate(id)}
          onNewTime={(dbName) => {
            if (dbName && dbName.toLowerCase() !== activeConnection.database.toLowerCase()) {
              switchDbMutation.mutate({ name: dbName });
            }
            setCreateTimeOpen(true);
          }}
          onSelectTime={(_t) => openTab("history", "Temporal Plane", "◴")}
          onDeleteTime={(id) => deleteTimeMutation.mutate(id)}
          onNewBranch={(dbName) => {
            if (dbName && dbName.toLowerCase() !== activeConnection.database.toLowerCase()) {
              switchDbMutation.mutate({ name: dbName });
            }
            setCreateBranchOpen(true);
          }}
          onSelectBranch={(_b) => openTab("causality", "Branches & DAG", "⑂")}
          onDeleteBranch={(name, dbName) => deleteBranchMutation.mutate({ name, dbName })}
          onOpenStorage={() => openTab("metrics", "Storage & Health", "▥")}
        />

        <div className="studio-main-workspace">
          <NavicatTabsBar
            tabs={tabs}
            activeTabId={activeTabId}
            onSelectTab={(id) => setActiveTabId(id)}
            onCloseTab={handleCloseTab}
            onNewQueryTab={() => handleNewQueryTab()}
          />

          <main className="studio-content">
            <React.Suspense fallback={<StudioLoadingFallback />}>
              {tabContent}
            </React.Suspense>
          </main>

          <NavicatStatusBar connection={activeConnection} eventCount={status.eventCount} />
        </div>
      </div>

      <React.Suspense fallback={null}>
        {editingConnection && (
          <ConnectionPropertiesModal
            connection={editingConnection}
            isOpen={true}
            onClose={() => setEditingConnection(null)}
            onSave={(updated) => saveMutation.mutate(updated)}
          />
        )}

        <CreateDatabaseModal
          isOpen={createDbOpen}
          connections={connections}
          initialConnectionId={createDbInitialConnId}
          onClose={() => setCreateDbOpen(false)}
          onCreate={(opts) => createDbMutation.mutate(opts)}
        />

        <CreateTableModal
          isOpen={createTableOpen}
          activeDatabase={activeConnection.database}
          onClose={() => setCreateTableOpen(false)}
          onCreate={(opts) => createTableMutation.mutate(opts)}
        />

        <CreateQueryModal
          isOpen={createQueryOpen}
          activeDatabase={activeConnection.database}
          onClose={() => setCreateQueryOpen(false)}
          onCreate={(opts) => createQueryMutation.mutate(opts)}
        />

        <CreateTimeModal
          isOpen={createTimeOpen}
          activeDatabase={activeConnection.database}
          onClose={() => setCreateTimeOpen(false)}
          onCreate={(opts) => createTimeMutation.mutate(opts)}
        />

        <CreateBranchModal
          isOpen={createBranchOpen}
          activeDatabase={activeConnection.database}
          existingBranches={branchObjectsData}
          onClose={() => setCreateBranchOpen(false)}
          onCreate={(opts) => createBranchMutation.mutate(opts)}
        />
      </React.Suspense>
    </div>
  );
}
