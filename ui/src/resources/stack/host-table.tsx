import {
  useDashboardPreferences,
  useRead,
  useResourceSelectionState,
} from "@/lib/hooks";
import ResourceLink from "@/resources/link";
import { DataTable, SortableHeader, hexColorByIntention } from "mogh_ui";
import { Types } from "komodo_client";
import { useMemo, useState } from "react";
import { BoxProps, Group, Text } from "@mantine/core";
import { SortingState } from "@tanstack/react-table";
import { ChevronDown, ChevronRight } from "lucide-react";
import TableTags from "@/components/tags/table";
import { ICONS } from "@/lib/icons";
import { UsageCell, UsageKind, usageSortValue } from "@/components/usage";
import {
  ServerHost,
  agentLabel,
  hostIntention,
} from "@/resources/server/hosts";
import AgentBadge from "@/resources/server/agent-badge";
import { StackComponents } from ".";
import {
  HostUsageCell,
  serverHostSize,
  sumStackUsage,
  useServerHosts,
} from "./usage";

/** The stacks on one host. `host` is missing for stacks whose
 * Server is not in the list, eg one the user can't see. */
interface StackHostGroup {
  name: string;
  host?: ServerHost;
  stacks: Types.StackListItem[];
  cpu: number | undefined;
  mem: number | undefined;
}

/** A host, or when `stack` is set, one stack row under an expanded host. */
interface HostStackRow {
  group: StackHostGroup;
  stack?: Types.StackListItem;
}

const UNKNOWN_HOST = "Unknown";

function stackServer(host: ServerHost | undefined, stack: Types.StackListItem) {
  return host?.servers.find((s) => s.id === stack.info.server_id);
}

export default function HostStackTable({
  resources,
  ...boxProps
}: {
  resources: Types.StackListItem[];
} & BoxProps) {
  const showStats = !!useDashboardPreferences().preferences.showStackStats;
  const selectionState = useResourceSelectionState("Stack");
  const { byServer } = useServerHosts();
  const coreVersion = useRead("GetVersion", {}).data?.version;
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [sorting, setSorting] = useState<SortingState>([]);

  const groups = useMemo(() => {
    const map = new Map<string, StackHostGroup>();
    for (const stack of resources) {
      const host = byServer.get(stack.info.server_id);
      const name = host?.name ?? UNKNOWN_HOST;
      const group = map.get(name);
      if (group) {
        group.stacks.push(stack);
      } else {
        map.set(name, {
          name,
          host,
          stacks: [stack],
          cpu: undefined,
          mem: undefined,
        });
      }
    }
    const groups = [...map.values()];
    for (const group of groups) {
      group.cpu = sumStackUsage(group.stacks, "cpu");
      group.mem = sumStackUsage(group.stacks, "mem");
    }

    const sort = sorting[0];
    const usageKind: UsageKind | undefined =
      sort?.id === "Cpu" ? "cpu" : sort?.id === "Memory" ? "mem" : undefined;
    const sign = sort?.desc ? -1 : 1;
    const compare = (a: string | number, b: string | number) =>
      typeof a === "number" && typeof b === "number"
        ? a - b
        : String(a).localeCompare(String(b));

    const stackKey = (stack: Types.StackListItem): string | number => {
      if (usageKind) return usageSortValue(stack.info.usage, usageKind);
      if (sort?.id === "Agent") {
        const server = stackServer(byServer.get(stack.info.server_id), stack);
        return server ? agentLabel(server) : "";
      }
      if (sort?.id === "State") return stack.info.state;
      return stack.name;
    };
    const groupKey = (group: StackHostGroup): string | number => {
      if (usageKind) return (usageKind === "cpu" ? group.cpu : group.mem) ?? -1;
      if (sort?.id === "State") return group.stacks.length;
      return group.name;
    };

    for (const group of groups) {
      group.stacks.sort((a, b) => {
        const cmp = compare(stackKey(a), stackKey(b));
        return (sort ? sign : 1) * (cmp || a.name.localeCompare(b.name));
      });
    }
    return groups.sort((a, b) => {
      // Stacks without a known host go last.
      if (!a.host !== !b.host) return a.host ? -1 : 1;
      const cmp = compare(groupKey(a), groupKey(b));
      return (sort ? sign : 1) * (cmp || a.name.localeCompare(b.name));
    });
  }, [resources, byServer, sorting]);

  const rows = useMemo(
    () =>
      groups.flatMap((group): HostStackRow[] =>
        expanded.has(group.name)
          ? [{ group }, ...group.stacks.map((stack) => ({ group, stack }))]
          : [{ group }],
      ),
    [groups, expanded],
  );

  const toggle = (name: string) =>
    setExpanded((prev) => {
      const next = new Set(prev);
      if (!next.delete(name)) next.add(name);
      return next;
    });

  const size = (row: HostStackRow) => serverHostSize(row.group.host);

  return (
    <DataTable
      {...boxProps}
      tableKey="host-stack-table"
      data={rows}
      manualSorting
      onSortingStateChange={setSorting}
      onRowClick={(row) => {
        if (!row.stack) toggle(row.group.name);
      }}
      rowProps={(row) =>
        row.stack
          ? { style: { backgroundColor: "var(--mantine-color-default-hover)" } }
          : { style: { cursor: "pointer" } }
      }
      selectOptions={{
        selectKey: (row) => row.stack?.name ?? `host ${row.group.name}`,
        state: selectionState,
        // A host row is not a Stack, its stacks are.
        canSelectRow: (row) => !!row.original.stack,
      }}
      columns={[
        {
          id: "Expand",
          enableSorting: false,
          header: "",
          cell: ({ row }) =>
            !row.original.stack &&
            (expanded.has(row.original.group.name) ? (
              <ChevronDown size="1rem" />
            ) : (
              <ChevronRight size="1rem" />
            )),
        },
        {
          id: "Name",
          accessorFn: (row) => row.group.name,
          header: ({ column }) => (
            <SortableHeader column={column} title="Host / Stack" />
          ),
          cell: ({ row: { original: row } }) => {
            if (row.stack) {
              return (
                <ResourceLink
                  type="Stack"
                  id={row.stack.id}
                  groupProps={{ pl: "md" }}
                />
              );
            }
            return (
              <Group gap="xs" wrap="nowrap">
                <ICONS.Server
                  size="1rem"
                  color={
                    row.group.host &&
                    hexColorByIntention(
                      hostIntention(row.group.host, coreVersion),
                    )
                  }
                />
                <Text
                  fz={{ base: "sm", lg: "md" }}
                  fw="bold"
                  style={{ textWrap: "nowrap" }}
                >
                  {row.group.name}
                </Text>
              </Group>
            );
          },
        },
        {
          id: "Agent",
          accessorFn: (row) => row.group.name,
          header: ({ column }) => (
            <SortableHeader column={column} title="Agent" />
          ),
          cell: ({ row: { original: row } }) => {
            if (row.stack) {
              const server = stackServer(row.group.host, row.stack);
              return server && <AgentBadge server={server} />;
            }
            const servers = row.group.host?.servers ?? [];
            const rootless = servers.filter((s) => s.info.rootless_user).length;
            const root = servers.length - rootless;
            return (
              <Text c="dimmed" style={{ textWrap: "nowrap" }}>
                {[root && "root", rootless && `${rootless} rootless`]
                  .filter((s) => s)
                  .join(" + ")}
              </Text>
            );
          },
        },
        showStats && {
          id: "Cpu",
          accessorFn: (row) => row.group.name,
          header: ({ column }) => (
            <SortableHeader column={column} title="CPU" sortDescFirst />
          ),
          cell: ({ row: { original: row } }) =>
            row.stack ? (
              <UsageCell
                usage={row.stack.info.usage}
                kind="cpu"
                of={size(row).cores}
              />
            ) : (
              <HostUsageCell
                value={row.group.cpu}
                kind="cpu"
                of={size(row).cores}
              />
            ),
        },
        showStats && {
          id: "Memory",
          accessorFn: (row) => row.group.name,
          header: ({ column }) => (
            <SortableHeader column={column} title="Memory" sortDescFirst />
          ),
          cell: ({ row: { original: row } }) =>
            row.stack ? (
              <UsageCell
                usage={row.stack.info.usage}
                kind="mem"
                of={size(row).memBytes}
              />
            ) : (
              <HostUsageCell
                value={row.group.mem}
                kind="mem"
                of={size(row).memBytes}
              />
            ),
        },
        {
          id: "State",
          accessorFn: (row) => row.group.name,
          header: ({ column }) => (
            <SortableHeader column={column} title="State" />
          ),
          cell: ({ row: { original: row } }) => {
            if (row.stack) return <StackComponents.State id={row.stack.id} />;
            const count = row.group.stacks.length;
            return (
              <Text c="dimmed" style={{ textWrap: "nowrap" }}>
                {count} stack{count !== 1 ? "s" : ""}
              </Text>
            );
          },
        },
        {
          id: "Tags",
          header: "Tags",
          cell: ({ row: { original: row } }) =>
            row.stack && <TableTags tagIds={row.stack.tags} />,
        },
      ]}
    />
  );
}
