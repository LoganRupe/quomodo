import { serverStateIntention } from "@/lib/color";
import { useListItem, useRead } from "@/lib/hooks";
import { ICONS } from "@/lib/icons";
import { RequiredResourceComponents } from "..";
import { Types } from "komodo_client";
import { StatusBadge } from "mogh_ui";
import ServerTable from "./table";
import NewResource from "@/resources/new";
import { hexColorByIntention } from "mogh_ui";
import { Prune } from "./executions";
import ServerVersion from "./version";
import { Box, Group, HoverCard } from "@mantine/core";
import { notifications } from "@mantine/notifications";
import ConfirmServerPubkey from "./confirm-pubkey";
import ServerTabs from "./tabs";
import { fmtUpperCamelcase } from "mogh_ui";
import ResourceHeader from "../header";
import { useIsServerAvailable } from "./hooks";
import BatchExecutions from "@/resources/batch-executions";
import { ServerLoadAverage } from "./stats/current/load-average";
import { ServerRamUsage } from "./stats/current/ram";
import ServerDiskUsage from "./diskUsage";
import ServerCpuUsage from "./stats/current/cpu";
import { HoverError } from "mogh_ui";

export function useServer(
  id: string | undefined,
  useName?: boolean,
  refetchInterval?: number | false,
) {
  return useListItem("Server", id, useName, refetchInterval);
}

export function useFullServer(id: string | undefined) {
  return useRead(
    "GetServer",
    { server: id! },
    { refetchInterval: 30_000, enabled: !!id },
  ).data;
}

export const ServerComponents: RequiredResourceComponents<
  Types.ServerConfig,
  Types.ServerInfo,
  Types.ServerListItemInfo,
  Types.ServerQuerySpecifics
> = {
  useList: (query, limit, page) =>
    useRead("ListServers", { query, limit, page }).data,
  useListItem: useServer,
  useFull: useFullServer,

  useResourceLinks: (server) => server?.config?.links,

  useDashboardSummaryData: () => {
    const summary = useRead(
      "GetServersSummary",
      {},
      { refetchInterval: 10_000 },
    ).data;
    return [
      { title: "Healthy", intention: "Good", value: summary?.healthy ?? 0 },
      {
        title: "Warning",
        intention: "Warning",
        value: summary?.warning ?? 0,
      },
      {
        title: "Unhealthy",
        intention: "Critical",
        value: summary?.unhealthy ?? 0,
      },
      {
        title: "Disabled",
        intention: "Neutral",
        value: summary?.disabled ?? 0,
      },
    ];
  },

  Description: () => (
    <>Connect servers for alerting and deploying.</>
  ),

  New: () => <NewResource type="Server" />,

  BatchExecutions: () => (
    <BatchExecutions
      type="Server"
      executions={[
        ["PruneImages", ICONS.Image],
        ["PruneSystem", ICONS.System],
      ]}
    />
  ),

  Table: ServerTable,

  Icon: ({ id, size = "1rem", noColor }) => {
    const coreVersion = useRead("GetVersion", {}).data?.version;
    const info = useServer(id)?.info;
    const state = info?.state;
    const color = noColor
      ? undefined
      : state &&
        hexColorByIntention(
          serverStateIntention(
            state,
            (info.version && coreVersion && info.version !== coreVersion) ||
              false,
          ),
        );
    return <ICONS.Server size={size} color={color} />;
  },

  ResourcePageHeader: ({ id }) => {
    const coreVersion = useRead("GetVersion", {}).data?.version;
    const server = useServer(id);
    const versionMismatch =
      !!coreVersion &&
      !!server?.info.version &&
      coreVersion !== server.info.version;
    return (
      <ResourceHeader
        type="Server"
        id={id}
        resource={server}
        intent={serverStateIntention(
          server?.info.state,
          !!coreVersion &&
            !!server?.info.version &&
            coreVersion !== server.info.version,
        )}
        icon={ICONS.Server}
        name={server?.name}
        state={
          server?.info.state === Types.ServerState.Ok && versionMismatch
            ? "Version Mismatch"
            : fmtUpperCamelcase(server?.info.state ?? "")
        }
        status={server?.info.region}
      />
    );
  },

  State: ({ id }) => {
    const coreVersion = useRead("GetVersion", {}).data?.version;
    const info = useServer(id)?.info;
    return (
      <StatusBadge
        text={info?.state}
        intent={serverStateIntention(
          info?.state,
          !!coreVersion && !!info?.version && coreVersion !== info.version,
        )}
      />
    );
  },
  Info: {
    ServerVersion,
    AgentUser: ({ id }) => {
      const info = useServer(id)?.info;
      if (!info?.version) return null;
      return (
        <HoverCard position="bottom-start">
          <HoverCard.Target>
            <Group gap="xs">
              <ICONS.User size="1rem" />
              {info.rootless_user ? `rootless (${info.rootless_user})` : "root"}
            </Group>
          </HoverCard.Target>
          <HoverCard.Dropdown>
            {info.rootless_user
              ? `The agent runs as ${info.rootless_user} and deploys that user's rootless quadlets`
              : "The agent runs as root and deploys rootful quadlets"}
          </HoverCard.Dropdown>
        </HoverCard>
      );
    },
    PublicIp: ({ id }) => {
      const publicIp = useServer(id)?.info.public_ip;
      return (
        <HoverCard position="bottom-start">
          <HoverCard.Target>
            <Group
              gap="xs"
              onClick={() => {
                publicIp &&
                  navigator.clipboard
                    .writeText(publicIp)
                    .then(() =>
                      notifications.show({ message: "Copied public IP" }),
                    );
              }}
              style={{ cursor: "pointer" }}
            >
              <ICONS.IP size="1rem" />
              {publicIp ?? "Unknown IP"}
            </Group>
          </HoverCard.Target>
          <HoverCard.Dropdown>Public IP (click to copy)</HoverCard.Dropdown>
        </HoverCard>
      );
    },
    Cpu: ({ id }) => {
      const isServerAvailable = useIsServerAvailable(id);
      const coreCount =
        useRead(
          "GetSystemInformation",
          { server: id },
          {
            enabled: isServerAvailable,
          },
        ).data?.core_count ?? 0;
      return (
        <HoverCard position="bottom-start">
          <HoverCard.Target>
            <Group gap="xs">
              <ICONS.Cpu size="1rem" />
              {coreCount
                ? `${coreCount} Core${coreCount === 1 ? "" : "s"}`
                : "N/A"}
            </Group>
          </HoverCard.Target>
          <HoverCard.Dropdown>
            <ServerCpuUsage id={id} />
          </HoverCard.Dropdown>
        </HoverCard>
      );
    },
    LoadAvg: ({ id }) => {
      const isServerAvailable = useIsServerAvailable(id);
      const stats = useRead(
        "GetSystemStats",
        { server: id },
        {
          enabled: isServerAvailable,
          refetchInterval: 5_000,
        },
      ).data;

      const one = stats?.load_average?.one;

      return (
        <HoverCard position="bottom-start">
          <HoverCard.Target>
            <Group gap="xs">
              <ICONS.LoadAvg size="1rem" />
              {one?.toFixed(2) ?? "N/A"}
            </Group>
          </HoverCard.Target>
          <HoverCard.Dropdown>
            <ServerLoadAverage id={id} stats={stats} />
          </HoverCard.Dropdown>
        </HoverCard>
      );
    },
    Memory: ({ id }) => {
      const isServerAvailable = useIsServerAvailable(id);
      const stats = useRead(
        "GetSystemStats",
        { server: id },
        {
          enabled: isServerAvailable,
          refetchInterval: 5_000,
        },
      ).data;
      return (
        <HoverCard position="bottom-start">
          <HoverCard.Target>
            <Group gap="xs">
              <ICONS.Memory size="1rem" />
              {stats?.mem_total_gb.toFixed(2).concat(" GB") ?? "N/A"}
            </Group>
          </HoverCard.Target>
          <HoverCard.Dropdown>
            <ServerRamUsage id={id} stats={stats} />
          </HoverCard.Dropdown>
        </HoverCard>
      );
    },
    Disk: ({ id }) => {
      const isServerAvailable = useIsServerAvailable(id);
      const stats = useRead(
        "GetSystemStats",
        { server: id },
        {
          enabled: isServerAvailable,
          refetchInterval: 5_000,
        },
      ).data;
      const diskTotalGb = stats?.disks.reduce(
        (acc, curr) => acc + curr.total_gb,
        0,
      );
      return (
        <HoverCard position="bottom-start">
          <HoverCard.Target>
            <Group gap="xs">
              <ICONS.Disk size="1rem" />
              {diskTotalGb?.toFixed(2).concat(" GB") ?? "N/A"}
            </Group>
          </HoverCard.Target>
          <HoverCard.Dropdown>
            <ServerDiskUsage id={id} stats={stats} withHeader />
          </HoverCard.Dropdown>
        </HoverCard>
      );
    },
    Err: ({ id }) => {
      const err = useServer(id)?.info.err;
      if (!err) return null;
      return (
        <Box>
          <HoverError {...err} />
        </Box>
      );
    },
    ConfirmServerPubkey,
  },

  Executions: {
    PruneImages: ({ id }) => <Prune serverId={id} type="Images" />,
    PruneSystem: ({ id }) => <Prune serverId={id} type="System" />,
  },

  Config: ServerTabs,

  Page: {},
};
