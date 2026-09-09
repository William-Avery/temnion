// SPDX-License-Identifier: AGPL-3.0-only

import type { StudioTab } from "../../types";

export interface NavicatTabsBarProps {
  tabs: StudioTab[];
  activeTabId: string;
  onSelectTab: (id: string) => void;
  onCloseTab: (id: string) => void;
  onNewQueryTab: () => void;
}

export function NavicatTabsBar({
  tabs,
  activeTabId,
  onSelectTab,
  onCloseTab,
  onNewQueryTab,
}: NavicatTabsBarProps) {
  return (
    <div className="navicat-tabs-bar" role="tablist" aria-label="Open documents">
      {tabs.map((tab) => {
        const isActive = tab.id === activeTabId;
        return (
          <div
            key={tab.id}
            role="tab"
            aria-selected={isActive}
            className={`navicat-tab ${isActive ? "active" : ""}`}
            onClick={() => onSelectTab(tab.id)}
          >
            <span>{tab.icon}</span>
            <span>{tab.title}</span>
            {tabs.length > 1 && (
              <button
                className="tab-close"
                type="button"
                title="Close tab"
                onClick={(e) => {
                  e.stopPropagation();
                  onCloseTab(tab.id);
                }}
              >
                ✕
              </button>
            )}
          </div>
        );
      })}
      <button
        className="tab-add-btn"
        type="button"
        title="New Query Tab"
        onClick={onNewQueryTab}
      >
        +
      </button>
    </div>
  );
}
