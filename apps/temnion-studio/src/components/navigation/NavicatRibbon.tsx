// SPDX-License-Identifier: AGPL-3.0-only

export interface NavicatRibbonProps {
  onNewConnection: () => void;
  onEditActiveConnection: () => void;
  onNewDatabase: () => void;
  onNewTable: () => void;
  onNewQuery: () => void;
  onNewTime: () => void;
  onNewBranch: () => void;
  onOpenIngest: () => void;
  onOpenStorage: () => void;
  onOpenGuide: () => void;
  onRefresh: () => void;
}

export function NavicatRibbon({
  onNewConnection,
  onEditActiveConnection,
  onNewDatabase,
  onNewTable,
  onNewQuery,
  onNewTime,
  onNewBranch,
  onOpenIngest,
  onOpenStorage,
  onOpenGuide,
  onRefresh,
}: NavicatRibbonProps) {
  return (
    <div className="navicat-ribbon" role="toolbar" aria-label="Navicat Action Ribbon">
      <div className="ribbon-group">
        <button className="ribbon-btn" type="button" onClick={onNewConnection} title="Create a new database connection">
          <span className="ribbon-icon">◎</span>
          <span className="ribbon-label">+ Connection</span>
        </button>
        <button className="ribbon-btn" type="button" onClick={onEditActiveConnection} title="Edit active connection properties">
          <span className="ribbon-icon">⚙</span>
          <span className="ribbon-label">Properties</span>
        </button>
        <button className="ribbon-btn" type="button" onClick={onNewDatabase} title="Create a new database under this connection">
          <span className="ribbon-icon" style={{ color: "#a5b4fc" }}>🗄️</span>
          <span className="ribbon-label">+ Database</span>
        </button>
      </div>

      <div className="ribbon-group">
        <button className="ribbon-btn" type="button" onClick={onNewTable} title="Create a new table under the active database">
          <span className="ribbon-icon" style={{ color: "#a5b4fc" }}>⊞</span>
          <span className="ribbon-label">+ Table</span>
        </button>
        <button className="ribbon-btn" type="button" onClick={onNewQuery} title="Open new query editor tab">
          <span className="ribbon-icon" style={{ color: "#38bdf8" }}>⚡</span>
          <span className="ribbon-label">+ Query</span>
        </button>
        <button className="ribbon-btn" type="button" onClick={onNewTime} title="Create a new time horizon or checkpoint">
          <span className="ribbon-icon" style={{ color: "#34d399" }}>◴</span>
          <span className="ribbon-label">+ Time</span>
        </button>
        <button className="ribbon-btn" type="button" onClick={onNewBranch} title="Create a new causal branch fork">
          <span className="ribbon-icon" style={{ color: "#c084fc" }}>⑂</span>
          <span className="ribbon-label">+ Branch</span>
        </button>
      </div>

      <div className="ribbon-group">
        <button className="ribbon-btn" type="button" onClick={onOpenIngest} title="Ingest bi-temporal event batches">
          <span className="ribbon-icon">⇧</span>
          <span className="ribbon-label">Ingest</span>
        </button>
        <button className="ribbon-btn" type="button" onClick={onOpenStorage} title="View storage metrics and segment manifests">
          <span className="ribbon-icon">▥</span>
          <span className="ribbon-label">Storage</span>
        </button>
        <button className="ribbon-btn" type="button" onClick={onRefresh} title="Refresh connection and cache">
          <span className="ribbon-icon">⟳</span>
          <span className="ribbon-label">Refresh</span>
        </button>
      </div>

      <div className="ribbon-group">
        <button className="ribbon-btn" type="button" onClick={onOpenGuide} title="Open developer How-To Guide">
          <span className="ribbon-icon" style={{ color: "#f59e0b" }}>📖</span>
          <span className="ribbon-label">Guide</span>
        </button>
      </div>
    </div>
  );
}
