// SPDX-License-Identifier: AGPL-3.0-only

import type { ReactNode } from "react";

export function Notice({
  kind = "info",
  children,
}: {
  kind?: "info" | "error" | "success";
  children: ReactNode;
}) {
  return <div className={`notice notice-${kind}`}>{children}</div>;
}
