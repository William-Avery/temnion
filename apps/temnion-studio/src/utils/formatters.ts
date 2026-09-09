// SPDX-License-Identifier: AGPL-3.0-only

export function formatBytes(bytes: number): string {
  if (bytes < 1_024) return `${bytes} B`;
  if (bytes < 1_048_576) return `${(bytes / 1_024).toFixed(1)} KiB`;
  return `${(bytes / 1_048_576).toFixed(1)} MiB`;
}

export function errorText(error: unknown): string {
  if (error instanceof Error) return error.message;
  return String(error);
}
