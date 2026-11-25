import { ColumnDef } from "@tanstack/react-table";
import { Types } from "komodo_client";
import { DataTableFeatures, SortableHeader } from "mogh_ui";
import { UsageCell, usageSortValue } from "@/components/usage";

/** CPU and Memory columns for a container table. Empty when no listed
 * container has usage, so tables look the same where nothing is
 * measured. Column ids are "Cpu" and "Memory". */
export function containerUsageColumns<
  T extends { usage?: Types.ContainerUsage },
>(rows: T[]): ColumnDef<DataTableFeatures, T, number>[] {
  if (!rows.some((row) => row.usage)) return [];
  return [
    {
      id: "Cpu",
      accessorFn: (row) => usageSortValue(row.usage, "cpu"),
      header: ({ column }) => (
        <SortableHeader column={column} title="CPU" sortDescFirst />
      ),
      cell: ({ row }) => <UsageCell usage={row.original.usage} kind="cpu" />,
    },
    {
      id: "Memory",
      accessorFn: (row) => usageSortValue(row.usage, "mem"),
      header: ({ column }) => (
        <SortableHeader column={column} title="Memory" sortDescFirst />
      ),
      cell: ({ row }) => (
        <UsageCell usage={row.original.usage} kind="mem" showLimit />
      ),
    },
  ];
}
