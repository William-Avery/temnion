// SPDX-License-Identifier: AGPL-3.0-only

import type { ReactNode } from "react";

export function EmptyState({ children }: { children: ReactNode }) {
  return <div className="empty-state">{children}</div>;
}
