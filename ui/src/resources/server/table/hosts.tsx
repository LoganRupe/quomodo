import {
  useDashboardPreferences,
  useRead,
  useResourceSelectionState,
} from "@/lib/hooks";
import ResourceLink from "@/resources/link";
import {
  DataTable,
  SortableHeader,
  StatusBadge,
  hexColorByIntention,
} from "mogh_ui";
import { Types } from "komodo_client";
import { useMemo, useState } from "react";
import { BoxProps, Group, Text } from "@mantine/core";
import { SortingState } from "@tanstack/react-table";
import { ChevronDown, ChevronRight } from "lucide-react";
import TableTags from "@/components/tags/table";
import { ICONS } from "@/lib/icons";
import { ServerComponents } from "..";
import ServerVersion from "../version";
import { CpuCell, DiskCell, MemCell } from "./stats";
import {
  ServerHost,
  diskPerc,
  groupServersByHost,
  hostIntention,
  hostStackCount,
  memPerc,
  useStacksByServer,
} from "../hosts";
import AgentBadge from "../agent-badge";

/** A host, or when `server` is set, one agent row under an expanded host. */
interface HostTableRow {
  host: ServerHost;
  server?: Types.ServerListItem;
}

/** A host with one agent is shown as that agent: no expand. */
function rowServer(row: HostTableRow) {
  return (
    row.server ??
    (row.host.servers.length === 1 ? row.host.servers[0] : undefined)
  );
}

export default function HostServerTable({
  resources,
  ...boxProps
}: {
  resources: Types.ServerListItem[];
} & BoxProps) {
  const { preferences } = useDashboardPreferences();
  const showStats = preferences.showServerStats;
  const selectionState = useResourceSelectionState("Server");
  const stacksByServer = useStacksByServer();
  const coreVersion = useRead("GetVersion", {}).data?.version;
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [sorting, setSorting] = useState<SortingState>([]);

  const hosts = useMemo(() => {
    const hosts = groupServersByHost(resources);
    const sort = sorting[0];
    if (!sort) return hosts;
    const key = (host: ServerHost): string | number => {
      const stats = host.primary.info.stats;
      switch (sort.id) {
        case "Agents":
          return host.servers.length;
        case "Stacks":
          return hostStackCount(host, stacksByServer);
        case "Cpu":
          return stats?.cpu_perc ?? -1;
        case "Memory":
          return memPerc(stats) ?? -1;
        case "Disk":
          return diskPerc(stats) ?? -1;
        case "State":
          return host.state;
        default:
          return host.name;
      }
    };
    return hosts.sort((a, b) => {
      const ka = key(a);
      const kb = key(b);
      const cmp =
        typeof ka === "number" && typeof kb === "number"
          ? ka - kb
          : String(ka).localeCompare(String(kb));
      return sort.desc ? -cmp : cmp;
    });
  }, [resources, sorting, stacksByServer]);

  const rows = useMemo(
    () =>
      hosts.flatMap((host): HostTableRow[] =>
        host.servers.length > 1 && expanded.has(host.name)
          ? [{ host }, ...host.servers.map((server) => ({ host, server }))]
          : [{ host }],
      ),
    [hosts, expanded],
  );

  const toggle = (name: string) =>
    setExpanded((prev) => {
      const next = new Set(prev);
      if (!next.delete(name)) next.add(name);
      return next;
    });

  return (
    <DataTable
      {...boxProps}
      tableKey="host-server-table"
      data={rows}
      manualSorting
      onSortingStateChange={setSorting}
      onRowClick={(row) => {
        if (!row.server && row.host.servers.length > 1) toggle(row.host.name);
      }}
      rowProps={(row) =>
        row.server
          ? { style: { backgroundColor: "var(--mantine-color-default-hover)" } }
          : row.host.servers.length > 1
            ? { style: { cursor: "pointer" } }
            : {}
      }
      selectOptions={{
        selectKey: (row) => rowServer(row)?.name ?? `host ${row.host.name}`,
        state: selectionState,
        // mogh_ui passes a function here straight to tanstack's
        // `enableRowSelection`, so it returns whether the row is selectable.
        // A multi agent host row is not a Server, its agents are.
        disableRow: (row) => !!rowServer(row.original),
      }}
      columns={[
        {
          id: "Expand",
          enableSorting: false,
          header: "",
          cell: ({ row }) =>
            !row.original.server &&
            row.original.host.servers.length > 1 &&
            (expanded.has(row.original.host.name) ? (
              <ChevronDown size="1rem" />
            ) : (
              <ChevronRight size="1rem" />
            )),
        },
        {
          id: "Name",
          accessorFn: (row) => row.host.name,
          header: ({ column }) => (
            <SortableHeader column={column} title="Host" />
          ),
          cell: ({ row: { original: row } }) => {
            const server = rowServer(row);
            if (row.server) {
              return (
                <ResourceLink
                  type="Server"
                  id={row.server.id}
                  groupProps={{ pl: "md" }}
                />
              );
            }
            if (server) {
              return <ResourceLink type="Server" id={server.id} />;
            }
            return (
              <Group gap="xs" wrap="nowrap">
                <ICONS.Server
                  size="1rem"
                  color={hexColorByIntention(
                    hostIntention(row.host, coreVersion),
                  )}
                />
                <Text
                  fz={{ base: "sm", lg: "md" }}
                  style={{ textWrap: "nowrap" }}
                >
                  {row.host.name}
                </Text>
              </Group>
            );
          },
        },
        {
          id: "Agents",
          accessorFn: (row) => row.host.servers.length,
          header: ({ column }) => (
            <SortableHeader column={column} title="Agents" />
          ),
          cell: ({ row: { original: row } }) => {
            const server = rowServer(row);
            if (server) return <AgentBadge server={server} />;
            const rootless = row.host.servers.filter(
              (s) => s.info.rootless_user,
            ).length;
            const root = row.host.servers.length - rootless;
            return (
              <Text c="dimmed" style={{ textWrap: "nowrap" }}>
                {[root && "root", rootless && `${rootless} rootless`]
                  .filter((s) => s)
                  .join(" + ")}
              </Text>
            );
          },
        },
        {
          id: "Stacks",
          accessorFn: (row) => row.host.name,
          header: ({ column }) => (
            <SortableHeader column={column} title="Stacks" />
          ),
          cell: ({ row: { original: row } }) => {
            const server = rowServer(row);
            if (!server) {
              return <Text>{hostStackCount(row.host, stacksByServer)}</Text>;
            }
            const stacks = stacksByServer.get(server.id) ?? [];
            return (
              <Group gap="xs" wrap="nowrap">
                <Text>{stacks.length}</Text>
                <Text c="dimmed" size="sm" style={{ textWrap: "nowrap" }}>
                  {stacks.join(", ")}
                </Text>
              </Group>
            );
          },
        },
        showStats && {
          id: "Cpu",
          accessorFn: (row) => row.host.name,
          header: ({ column }) => (
            <SortableHeader column={column} title="CPU" sortDescFirst />
          ),
          cell: ({ row: { original: row } }) =>
            !row.server && <CpuCell server={row.host.primary} />,
        },
        showStats && {
          id: "Memory",
          accessorFn: (row) => row.host.name,
          header: ({ column }) => (
            <SortableHeader column={column} title="Memory" sortDescFirst />
          ),
          cell: ({ row: { original: row } }) =>
            !row.server && <MemCell server={row.host.primary} />,
        },
        showStats && {
          id: "Disk",
          accessorFn: (row) => row.host.name,
          header: ({ column }) => (
            <SortableHeader column={column} title="Disk" sortDescFirst />
          ),
          cell: ({ row: { original: row } }) =>
            !row.server && <DiskCell server={row.host.primary} />,
        },
        {
          id: "Version",
          header: "Version",
          cell: ({ row: { original: row } }) => {
            const server = rowServer(row);
            if (server) return <ServerVersion id={server.id} />;
            if (row.host.mixedVersions) {
              return <Text c="dimmed">Mixed</Text>;
            }
            return <ServerVersion id={row.host.primary.id} />;
          },
        },
        {
          id: "State",
          accessorFn: (row) => row.host.state,
          header: ({ column }) => (
            <SortableHeader column={column} title="State" />
          ),
          cell: ({ row: { original: row } }) => {
            const server = rowServer(row);
            if (server) return <ServerComponents.State id={server.id} />;
            return (
              <StatusBadge
                text={row.host.state}
                intent={hostIntention(row.host, coreVersion)}
              />
            );
          },
        },
        !showStats && {
          id: "Tags",
          header: "Tags",
          cell: ({ row: { original: row } }) => {
            const server = rowServer(row);
            return server && <TableTags tagIds={server.tags} />;
          },
        },
      ]}
    />
  );
}
