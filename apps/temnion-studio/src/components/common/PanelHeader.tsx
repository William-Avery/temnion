// SPDX-License-Identifier: AGPL-3.0-only

import type { ReactNode } from "react";

export function PanelHeader({
  title,
  subtitle,
  action,
}: {
  title: string;
  subtitle: string;
  action?: ReactNode;
}) {
  return (
    <div className="panel-header">
      <div className="panel-title-group">
        <h2>{title}</h2>
        <p className="panel-subtitle">{subtitle}</p>
      </div>
      {action}
    </div>
  );
}
