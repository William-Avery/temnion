// SPDX-License-Identifier: AGPL-3.0-only

export type View =
  | "guide"
  | "query"
  | "history"
  | "causality"
  | "schemas"
  | "ingest"
  | "connections"
  | "metrics";

export type QueryFormat = "temql" | "compact" | "sql";

export interface StudioTab {
  id: string;
  type: View | "table";
  title: string;
  icon: string;
  tableName?: string;
  querySnippet?: string;
}

export const samples: Record<QueryFormat, string> = {
  temql: "FROM temnion\nSELECT entity, schema, valid_time, known_time, sequence\nLIMIT 100",
  compact: "tn:>entity,schema,valid_time,known_time,sequence!100",
  sql: "SELECT entity, schema, valid_time, known_time, sequence\nFROM temnion\nLIMIT 100",
};

export const navItems: Array<{ id: View; label: string; icon: string; group: string }> = [
  { id: "guide", label: "How-To Guide", icon: "📖", group: "Learn & Docs" },
  { id: "query", label: "Query Studio", icon: "⚡", group: "Workbench" },
  { id: "schemas", label: "Schema Catalog", icon: "⊞", group: "Workbench" },
  { id: "history", label: "Temporal Plane", icon: "◴", group: "Workbench" },
  { id: "causality", label: "Branches & DAG", icon: "⑂", group: "Workbench" },
  { id: "connections", label: "Connections", icon: "◎", group: "Operate" },
  { id: "ingest", label: "Ingestion Console", icon: "⇧", group: "Operate" },
  { id: "metrics", label: "Storage & Health", icon: "▥", group: "Operate" },
];
