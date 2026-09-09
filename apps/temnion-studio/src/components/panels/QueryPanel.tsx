// SPDX-License-Identifier: AGPL-3.0-only
import { useEffect, useRef, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";

import { executeQuery, explainQuery, subscribeLive, type LiveStreamEvent } from "../../api";
import { type QueryFormat, samples } from "../../types";
import { errorText, formatBytes } from "../../utils/formatters";
import { EventTable, Notice, PanelHeader } from "../common";
import { QueryEditor } from "../../QueryEditor";

export interface QueryPanelProps {
  connected: boolean;
  initialQuery?: string;
  onNavigateToGuide: () => void;
  onNavigateToConnections: () => void;
}

export const queryPresets = [
  {
    label: "Basic Scan (SQL)",
    fmt: "sql" as QueryFormat,
    text: "SELECT entity, schema, valid_time, known_time, sequence\nFROM temnion\nLIMIT 25",
  },
  {
    label: "Live Subscription (SQL)",
    fmt: "sql" as QueryFormat,
    text: "SELECT entity, schema, valid_time, known_time, sequence\nFROM temnion\nWHERE schema = 1\nLIMIT 50",
  },
  {
    label: "Time Travel Flashback (SQL)",
    fmt: "sql" as QueryFormat,
    text: "SELECT entity, schema, valid_time, known_time, sequence\nFROM temnion\nAS OF SYSTEM_TIME '2026-09-08T00:00:00Z'\nLIMIT 50",
  },
  {
    label: "Physical Range (SQL)",
    fmt: "sql" as QueryFormat,
    text: "SELECT entity, schema, valid_time, known_time, sequence\nFROM temnion\nBETWEEN PHYSICAL 1757300000000000000 AND 1757305000000000000\nLIMIT 50",
  },
  {
    label: "Create Database (SQL)",
    fmt: "sql" as QueryFormat,
    text: "CREATE DATABASE telemetry_stream;",
  },
  {
    label: "Show Databases (SQL)",
    fmt: "sql" as QueryFormat,
    text: "SHOW DATABASES;",
  },
  {
    label: "TemQL Pipeline",
    fmt: "temql" as QueryFormat,
    text: "FROM temnion\nWHERE sequence > 0\nSELECT entity, schema, valid_time, known_time, sequence\nORDER BY sequence DESC\nLIMIT 25",
  },
];

export function QueryPanel({
  connected,
  initialQuery,
  onNavigateToGuide,
  onNavigateToConnections,
}: QueryPanelProps) {
  const [format, setFormat] = useState<QueryFormat>(() => {
    if (initialQuery?.trim().toUpperCase().startsWith("SELECT")) return "sql";
    return "temql";
  });
  const [query, setQuery] = useState(initialQuery ?? samples.temql);
  const [maxRows, setMaxRows] = useState(100);
  const [bookmarks, setBookmarks] = useState<string[]>(() => {
    try {
      return JSON.parse(localStorage.getItem("temnion-query-bookmarks") ?? "[]") as string[];
    } catch {
      return [];
    }
  });

  // Live Subscription Streaming State
  const [isStreaming, setIsStreaming] = useState(false);
  const [isPaused, setIsPaused] = useState(false);
  const [autoScroll, setAutoScroll] = useState(true);
  const [streamEvents, setStreamEvents] = useState<LiveStreamEvent[]>([]);
  const [activeResultTab, setActiveResultTab] = useState<"static" | "stream">("static");
  const unsubscribeRef = useRef<(() => void) | null>(null);
  const streamBottomRef = useRef<HTMLDivElement | null>(null);

  const startStreaming = () => {
    if (unsubscribeRef.current) {
      unsubscribeRef.current();
      unsubscribeRef.current = null;
    }
    setStreamEvents([]);
    setIsStreaming(true);
    setIsPaused(false);
    setActiveResultTab("stream");

    const unsub = subscribeLive(
      {
        query,
        format,
        fromSequence: 0,
        fromNow: false,
      },
      (event) => {
        setStreamEvents((prev) => {
          if (isPaused) return prev;
          return [event, ...prev].slice(0, 500);
        });
      }
    );
    unsubscribeRef.current = unsub;
  };

  const stopStreaming = () => {
    if (unsubscribeRef.current) {
      unsubscribeRef.current();
      unsubscribeRef.current = null;
    }
    setIsStreaming(false);
  };

  const clearStream = () => {
    setStreamEvents([]);
  };

  useEffect(() => {
    return () => {
      if (unsubscribeRef.current) {
        unsubscribeRef.current();
      }
    };
  }, []);

  useEffect(() => {
    if (autoScroll && streamBottomRef.current) {
      streamBottomRef.current.scrollIntoView({ behavior: "smooth" });
    }
  }, [streamEvents, autoScroll]);

  useEffect(() => {
    if (initialQuery) {
      setQuery(initialQuery);
      if (initialQuery.trim().toUpperCase().startsWith("SELECT")) {
        setFormat("sql");
      } else if (initialQuery.trim().toUpperCase().startsWith("FROM")) {
        setFormat("temql");
      }
    }
  }, [initialQuery]);

  const queryClient = useQueryClient();
  const run = useMutation({
    mutationFn: () => executeQuery(query, maxRows),
    onSuccess: (data) => {
      if (data.message) {
        queryClient.invalidateQueries({ queryKey: ["databases"] });
        queryClient.invalidateQueries({ queryKey: ["connections"] });
        queryClient.invalidateQueries({ queryKey: ["tables"] });
        queryClient.invalidateQueries({ queryKey: ["saved-queries"] });
        queryClient.invalidateQueries({ queryKey: ["time-objects"] });
        queryClient.invalidateQueries({ queryKey: ["branch-objects"] });
      }
    },
  });
  const explain = useMutation({ mutationFn: () => explainQuery(query) });

  const chooseFormat = (next: QueryFormat) => {
    setFormat(next);
    setQuery(samples[next]);
    run.reset();
    explain.reset();
  };

  const saveBookmark = () => {
    const next = [query, ...bookmarks.filter((value) => value !== query)].slice(0, 8);
    setBookmarks(next);
    localStorage.setItem("temnion-query-bookmarks", JSON.stringify(next));
  };

  return (
    <section className="view-panel active">
      <PanelHeader
        title="Query Studio"
        subtitle="TemQL, compact Tem, and SQL lower through the same canonical typed query IR."
        action={
          <div className="frontend-tabs" aria-label="Query language">
            {(["temql", "compact", "sql"] as QueryFormat[]).map((item) => (
              <button
                className={`frontend-tab ${format === item ? "active" : ""}`}
                key={item}
                onClick={() => chooseFormat(item)}
                type="button"
              >
                {item === "compact" ? "Compact tn:" : item.toUpperCase()}
              </button>
            ))}
          </div>
        }
      />

      {/* Quickstart Hero Card */}
      <div className="quickstart-hero">
        <div className="quickstart-text">
          <h3>Interactive Temporal Query Studio</h3>
          <p>
            Execute SQL or native TemQL queries across physical wall-clock time, Lamport sequences, and causal branch DAGs. All operations are bounded and verified memory-safe under <code>#![forbid(unsafe_code)]</code>.
          </p>
        </div>
        <div className="quickstart-actions">
          <button className="btn btn-secondary btn-sm" onClick={onNavigateToGuide} type="button">
            📖 How-To Guide
          </button>
          <button className="btn btn-secondary btn-sm" onClick={onNavigateToConnections} type="button">
            ◎ Connections
          </button>
          <button
            className="btn btn-primary btn-sm"
            onClick={() => {
              setFormat("sql");
              setQuery(queryPresets[1].text);
              run.mutate();
            }}
            type="button"
          >
            ⚡ Run Time Travel Demo
          </button>
        </div>
      </div>

      {!connected && <Notice>Connect an existing database or create one before executing queries.</Notice>}

      {/* Query Presets Chips */}
      <div className="query-presets-bar">
        <span className="presets-label">Sample Presets:</span>
        {queryPresets.map((p) => (
          <button
            className="preset-chip"
            key={p.label}
            onClick={() => {
              setFormat(p.fmt);
              setQuery(p.text);
              run.reset();
              explain.reset();
            }}
            type="button"
          >
            {p.label}
          </button>
        ))}
      </div>

      <div className="editor-container glass-card">
        <div className="editor-toolbar">
          <div className="toolbar-left">
            <span className="lang-tag">{format}</span>
            <button className="tool-btn" type="button" onClick={() => setQuery(samples[format])}>Load sample</button>
            <button className="tool-btn" type="button" onClick={saveBookmark}>Bookmark</button>
          </div>
          <div className="toolbar-right bounded-label">Hard result ceiling: 1,000 rows</div>
        </div>
        <QueryEditor
          ariaLabel="Query text"
          language={format}
          onChange={setQuery}
          value={query}
        />
        <div className="editor-footer">
          <label className="budget-label">
            Max rows
            <input
              className="num-input"
              max={1_000}
              min={1}
              type="number"
              value={maxRows}
              onChange={(event) => setMaxRows(Number(event.target.value))}
            />
          </label>
          <div className="exec-actions">
            <button className="btn btn-secondary" disabled={explain.isPending} onClick={() => explain.mutate()} type="button">
              {explain.isPending ? "Planning…" : "Explain"}
            </button>
            <button
              className="btn btn-primary"
              disabled={!connected || run.isPending}
              onClick={() => {
                setActiveResultTab("static");
                run.mutate();
              }}
              type="button"
            >
              {run.isPending ? "Executing…" : "Execute query"}
            </button>
            {isStreaming ? (
              <button className="btn btn-danger" onClick={stopStreaming} type="button">
                ⏹ Stop Stream
              </button>
            ) : (
              <button
                className="btn btn-success"
                disabled={!connected}
                onClick={startStreaming}
                type="button"
                title="Subscribe to live events with filter pushdown"
              >
                <span className="live-pulse" /> ⚡ Live Stream
              </button>
            )}
          </div>
        </div>
      </div>
      {(run.error || explain.error) && <Notice kind="error">{errorText(run.error ?? explain.error)}</Notice>}
      {explain.data && <pre className="explain-output glass-card">{explain.data}</pre>}
      {run.data?.message && (
        <div className="query-result-message glass-card">
          <span>{run.data.message}</span>
        </div>
      )}

      {/* Result Tabs Selector */}
      <div className="frontend-tabs" style={{ margin: "0.5rem 0 1rem 0" }} aria-label="Query results mode">
        <button
          className={`frontend-tab ${activeResultTab === "static" ? "active" : ""}`}
          onClick={() => setActiveResultTab("static")}
          type="button"
        >
          ⚡ Query Results ({run.data?.rows.length ?? 0})
        </button>
        <button
          className={`frontend-tab ${activeResultTab === "stream" ? "active" : ""}`}
          onClick={() => setActiveResultTab("stream")}
          type="button"
        >
          <span className={`live-pulse ${isStreaming && !isPaused ? "" : "paused"}`} />
          Live Stream ({streamEvents.length})
        </button>
      </div>

      {activeResultTab === "stream" ? (
        <div className="results-container glass-card">
          <div className="results-header">
            <div className="stream-controls-bar" style={{ width: "100%" }}>
              <div className="results-stats" style={{ display: "flex", alignItems: "center", gap: "10px", flexWrap: "wrap" }}>
                {isStreaming ? (
                  <span className={`stream-meta-badge ${isPaused ? "paused" : ""}`}>
                    <span className={`live-pulse ${isPaused ? "paused" : ""}`} />
                    {isPaused ? "STREAM PAUSED" : "LIVE STREAMING"}
                  </span>
                ) : (
                  <span className="stream-meta-badge paused">STREAM STOPPED</span>
                )}
                <span><strong>{streamEvents.length}</strong> events</span>
                <span>•</span>
                <span style={{ color: "#34d399" }}><strong>{streamEvents.filter((e) => e.isLive).length}</strong> live</span>
                <span>•</span>
                <span style={{ color: "#60a5fa" }}><strong>{streamEvents.filter((e) => !e.isLive).length}</strong> snapshot</span>
                <span className="stat-pill safety">forbid(unsafe_code)</span>
              </div>
              <div style={{ display: "flex", alignItems: "center", gap: "10px" }}>
                {isStreaming && (
                  <button
                    className="btn btn-secondary btn-sm"
                    type="button"
                    onClick={() => setIsPaused(!isPaused)}
                  >
                    {isPaused ? "▶ Resume" : "❚❚ Pause"}
                  </button>
                )}
                <label style={{ fontSize: "12px", display: "flex", alignItems: "center", gap: "4px", cursor: "pointer", color: "#94a3b8" }}>
                  <input
                    type="checkbox"
                    checked={autoScroll}
                    onChange={(e) => setAutoScroll(e.target.checked)}
                  />
                  Auto-scroll
                </label>
                <button className="btn btn-secondary btn-sm" type="button" onClick={clearStream}>
                  Clear
                </button>
              </div>
            </div>
          </div>
          {streamEvents.length > 0 ? (
            <div className="stream-table-container">
              <table className="event-table">
                <thead>
                  <tr>
                    <th>Mode</th>
                    <th>Seq</th>
                    <th>Entity</th>
                    <th>Schema</th>
                    <th>Valid Time</th>
                    <th>Known Time</th>
                    <th>Payload Hex</th>
                    <th>Received At</th>
                  </tr>
                </thead>
                <tbody>
                  {streamEvents.map((ev, idx) => (
                    <tr key={`${ev.subscriptionId}-${ev.sequence}-${idx}`}>
                      <td>
                        <span className={`badge ${ev.isLive ? "badge-live" : "badge-snapshot"}`}>
                          {ev.isLive ? "LIVE" : "SNAPSHOT"}
                        </span>
                      </td>
                      <td><strong>#{ev.sequence}</strong></td>
                      <td><code>{ev.entity}</code></td>
                      <td><span className="badge badge-info">Schema {ev.schema}</span></td>
                      <td><code>{ev.validClock}:{ev.validTime}</code></td>
                      <td><code>{ev.knownClock}:{ev.knownTime}</code></td>
                      <td><code style={{ color: "#a5b4fc" }}>{ev.payloadHex || "0x00"}</code></td>
                      <td style={{ color: "#94a3b8", fontSize: "11px" }}>{ev.receivedAt}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
              <div ref={streamBottomRef} />
            </div>
          ) : (
            <div style={{ padding: "40px", textAlign: "center", color: "#94a3b8" }}>
              <span className="live-pulse" /> {isStreaming ? "Listening for real-time subscription events..." : "Click '⚡ Live Stream' to start live subscription streaming."}
            </div>
          )}
        </div>
      ) : (
        <div className="results-container glass-card">
          <div className="results-header">
            <div className="results-stats">
              <span><strong>{run.data?.rows.length ?? 0}</strong> rows</span>
              <span>•</span>
              <span><strong>{run.data?.eventsScanned ?? 0}</strong> scanned</span>
              <span>•</span>
              <span><strong>{formatBytes(run.data?.bytesRead ?? 0)}</strong> read</span>
              <span>•</span>
              <span>{((run.data?.elapsedMicros ?? 0) / 1_000).toFixed(2)} ms</span>
              <span className="stat-pill safety">forbid(unsafe_code)</span>
              {run.data?.truncated && <span className="badge badge-warning">Truncated</span>}
            </div>
          </div>
          <EventTable rows={run.data?.rows ?? []} />
        </div>
      )}
      {!!bookmarks.length && (
        <div className="glass-card bookmark-card">
          <div className="card-title">Local query bookmarks</div>
          {bookmarks.map((bookmark, index) => (
            <button className="bookmark" key={`${bookmark}-${index}`} onClick={() => setQuery(bookmark)} type="button">
              <code>{bookmark.replaceAll("\n", " ")}</code>
            </button>
          ))}
        </div>
      )}
    </section>
  );
}
