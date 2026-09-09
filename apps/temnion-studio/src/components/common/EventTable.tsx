// SPDX-License-Identifier: AGPL-3.0-only

import { useMemo } from "react";
import {
  type ColumnDef,
  flexRender,
  getCoreRowModel,
  useReactTable,
} from "@tanstack/react-table";

import type { EventRow } from "../../api";
import { EmptyState } from "./EmptyState";

export function EventTable({
  rows,
  showPayload = false,
}: {
  rows: EventRow[];
  showPayload?: boolean;
}) {
  const columns = useMemo<ColumnDef<EventRow>[]>(() => {
    const base: ColumnDef<EventRow>[] = [
      { accessorKey: "sequence", header: "Sequence" },
      { accessorKey: "entity", header: "Entity" },
      { accessorKey: "schema", header: "Schema" },
      {
        id: "validTime",
        header: "Valid time",
        cell: ({ row }) => `${row.original.validClock}:${row.original.validTime}`,
      },
      {
        id: "knownTime",
        header: "Known time",
        cell: ({ row }) => `${row.original.knownClock}:${row.original.knownTime}`,
      },
      {
        id: "fields",
        header: "Fields",
        cell: ({ row }) =>
          row.original.fields.length
            ? row.original.fields.map((field) => `${field.name}=${field.value}`).join(", ")
            : "—",
      },
    ];
    if (showPayload) {
      base.push({
        id: "payload",
        header: "Payload preview",
        cell: ({ row }) => `${row.original.payloadHex || "—"} (${row.original.payloadBytes} B)`,
      });
      base.push({
        id: "causes",
        header: "Causes",
        cell: ({ row }) => row.original.causes.join(", ") || "—",
      });
    }
    return base;
  }, [showPayload]);

  const table = useReactTable({ data: rows, columns, getCoreRowModel: getCoreRowModel() });

  if (!rows.length) {
    return <EmptyState>No rows loaded. Results remain empty until a bounded native query completes.</EmptyState>;
  }

  return (
    <div className="table-scroll">
      <table className="studio-table">
        <thead>
          {table.getHeaderGroups().map((group) => (
            <tr key={group.id}>
              {group.headers.map((header) => (
                <th key={header.id}>
                  {header.isPlaceholder ? null : flexRender(header.column.columnDef.header, header.getContext())}
                </th>
              ))}
            </tr>
          ))}
        </thead>
        <tbody>
          {table.getRowModel().rows.map((row) => (
            <tr key={row.id}>
              {row.getVisibleCells().map((cell) => (
                <td key={cell.id}>{flexRender(cell.column.columnDef.cell, cell.getContext())}</td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
