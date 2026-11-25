import {
  useDashboardPreferences,
  useResourceSelectionState,
  useStackView,
} from "@/lib/hooks";
import { DataTable, DataTableFeatures, SortableHeader } from "mogh_ui";
import { BoxProps } from "@mantine/core";
import { ColumnDef } from "@tanstack/react-table";
import { Types } from "komodo_client";
import ResourceLink from "@/resources/link";
import { StackComponents, StackSource } from ".";
import TableTags from "@/components/tags/table";
import { UsageCell, UsageKind, usageSortValue } from "@/components/usage";
import { serverHostSize, useServerHosts } from "./usage";
import HostStackTable from "./host-table";

const SORT_KEYS: string[] = Object.values(Types.StackSortBy);

type Column = ColumnDef<DataTableFeatures, Types.StackListItem, any>;

export default function StackTable({
  resources,
  onServerSort,
  ...boxProps
}: {
  resources: Types.StackListItem[];
  /** When provided, sorting is handled server side,
   * and sort updates are passed to this callback. */
  onServerSort?: (sort: { sort_by?: string; sort_desc?: boolean }) => void;
} & BoxProps) {
  const [view] = useStackView();
  // The host table groups and sorts on the client.
  if (view === "hosts") {
    return <HostStackTable resources={resources} {...boxProps} />;
  }
  return (
    <StandardStackTable
      resources={resources}
      onServerSort={onServerSort}
      {...boxProps}
    />
  );
}

function StandardStackTable({
  resources,
  onServerSort,
  ...boxProps
}: {
  resources: Types.StackListItem[];
  onServerSort?: (sort: { sort_by?: string; sort_desc?: boolean }) => void;
} & BoxProps) {
  const selectionState = useResourceSelectionState("Stack");
  const showStats = !!useDashboardPreferences().preferences.showStackStats;
  const { byServer } = useServerHosts();

  const sourceColumn: Column = {
    header: ({ column }) => <SortableHeader column={column} title="Source" />,
    id: "Source",
    accessorKey: "info.repo",
    cell: ({ row }) => <StackSource info={row.original.info} />,
  };

  const usageColumn = (id: string, title: string, kind: UsageKind): Column => ({
    id,
    accessorFn: (row) => usageSortValue(row.info.usage, kind),
    header: ({ column }) => (
      <SortableHeader column={column} title={title} sortDescFirst />
    ),
    cell: ({ row }) => {
      const size = serverHostSize(byServer.get(row.original.info.server_id));
      return (
        <UsageCell
          usage={row.original.info.usage}
          kind={kind}
          of={kind === "cpu" ? size.cores : size.memBytes}
        />
      );
    },
  });

  return (
    <DataTable
      {...boxProps}
      manualSorting={!!onServerSort}
      onSortingStateChange={(sorting) => {
        if (!onServerSort) return;
        const sort = sorting.find((s) => SORT_KEYS.includes(s.id));
        onServerSort(sort ? { sort_by: sort.id, sort_desc: sort.desc } : {});
      }}
      tableKey="stack-table"
      data={resources}
      selectOptions={{
        selectKey: ({ name }) => name,
        state: selectionState,
      }}
      columns={[
        {
          header: ({ column }) => (
            <SortableHeader column={column} title="Name" />
          ),
          id: "Name",
          accessorKey: "name",
          cell: ({ row }) => <ResourceLink type="Stack" id={row.original.id} />,
        },
        !showStats && sourceColumn,
        {
          header: ({ column }) => (
            <SortableHeader column={column} title="Host" />
          ),
          id: "Host",
          accessorKey: "info.server_id",
          sortFn: (a, b) => {
            const name_a = a.original.info.server_name;
            const name_b = b.original.info.server_name;

            if (!name_a && !name_b) return 0;
            if (!name_a) return 1;
            if (!name_b) return -1;

            if (name_a > name_b) return 1;
            else if (name_a < name_b) return -1;
            else return 0;
          },
          cell: ({ row }) => (
            <ResourceLink type="Server" id={row.original.info.server_id} />
          ),
        },
        showStats && usageColumn("Cpu", "CPU", "cpu"),
        showStats && usageColumn("Memory", "Memory", "mem"),
        {
          id: "State",
          accessorKey: "info.state",
          header: ({ column }) => (
            <SortableHeader column={column} title="State" />
          ),
          cell: ({ row }) => <StackComponents.State id={row.original.id} />,
        },
        // With stats on, Source moves right of State, as on the design.
        showStats && sourceColumn,
        {
          header: "Tags",
          cell: ({ row }) => <TableTags tagIds={row.original.tags} />,
        },
      ]}
    />
  );
}
