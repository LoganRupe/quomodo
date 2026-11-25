import { useDashboardPreferences, useRead } from "@/lib/hooks";
import { useServer } from ".";
import { Types } from "komodo_client";
import { useMemo } from "react";
import { ICONS } from "@/lib/icons";
import {
  Center,
  Group,
  Overlay,
  Progress,
  Stack,
  Text,
  useComputedColorScheme,
} from "@mantine/core";
import { ColorIntention, hexColorByIntention } from "mogh_ui";
import { LucideIcon } from "lucide-react";
import { serverThresholds } from "./hooks";
import UsageBars from "./usage-bars";

export interface ServerStatsCardProps {
  id: string;
}

export default function ServerStatsCard({ id }: ServerStatsCardProps) {
  const { preferences } = useDashboardPreferences();
  const server = useServer(id);
  const isServerAvailable = server?.info.state === Types.ServerState.Ok;
  const enabled = preferences.showServerStats && isServerAvailable;

  const {
    cpuWarning,
    cpuCritical,
    memWarning,
    memCritical,
    diskWarning,
    diskCritical,
  } = serverThresholds(server);

  const intention = (percentage: number, type: "cpu" | "memory" | "disk") => {
    const warning =
      type === "cpu"
        ? cpuWarning
        : type === "memory"
          ? memWarning
          : diskWarning;
    const critical =
      type === "cpu"
        ? cpuCritical
        : type === "memory"
          ? memCritical
          : diskCritical;

    if (percentage >= critical) return "Critical";
    if (percentage >= warning) return "Warning";
    return "Good";
  };

  const stats = useRead(
    "GetSystemStats",
    { server: id },
    {
      enabled,
      refetchInterval: 15_000,
      staleTime: 5_000,
    },
  ).data;

  const calculations = useMemo(() => {
    const diskUsed = stats
      ? stats.disks.reduce((acc, disk) => acc + disk.used_gb, 0)
      : 0;
    const diskTotal = stats
      ? stats.disks.reduce((acc, disk) => acc + disk.total_gb, 0)
      : 0;

    return {
      cpuPercentage: stats?.cpu_perc ?? 0,
      memoryPercentage:
        stats && stats.mem_total_gb > 0
          ? (stats.mem_used_gb / stats.mem_total_gb) * 100
          : 0,
      diskPercentage: diskTotal > 0 ? (diskUsed / diskTotal) * 100 : 0,
      isUnreachable: !stats || server?.info.state === Types.ServerState.NotOk,
      isDisabled: server?.info.state === Types.ServerState.Disabled,
    };
  }, [stats, server?.info.state]);

  const {
    cpuPercentage,
    memoryPercentage,
    diskPercentage,
    isUnreachable,
    isDisabled,
  } = calculations;

  const statItems = useMemo(
    () => [
      {
        label: "CPU",
        icon: ICONS.Cpu,
        percentage: cpuPercentage,
        type: "cpu" as const,
      },
      {
        label: "Memory",
        icon: ICONS.Memory,
        percentage: memoryPercentage,
        type: "memory" as const,
      },
      {
        label: "Disk",
        icon: ICONS.Disk,
        percentage: diskPercentage,
        type: "disk" as const,
      },
    ],
    [cpuPercentage, memoryPercentage, diskPercentage],
  );
  const theme = useComputedColorScheme();

  if (!server) {
    return null;
  }

  return (
    <Stack
      gap="xs"
      p={isUnreachable || isDisabled ? "xs" : undefined}
      pos="relative"
    >
      {server.info.rootless_user ? (
        // A rootless agent's card shows its own containers. Every agent
        // on the machine reports the same host stats, so they go on one
        // line underneath instead of looking like this agent's numbers.
        <>
          <UsageBars usage={server.info.usage} server={server} />
          <Text size="xs" c="dimmed" ff="monospace">
            {[
              "host",
              ...statItems.map(
                (item) =>
                  `${item.type === "memory" ? "mem" : item.type} ${
                    isUnreachable || isDisabled
                      ? "N/A"
                      : `${item.percentage.toFixed(1)}%`
                  }`,
              ),
            ].join(" · ")}
          </Text>
        </>
      ) : (
        statItems.map((item) => (
          <StatItem
            key={item.label}
            isUnreachable={isUnreachable || isDisabled}
            intention={intention}
            {...item}
          />
        ))
      )}
      {(isUnreachable || isDisabled) && (
        <Center
          renderRoot={(props) => (
            <Overlay
              color="black"
              backgroundOpacity={theme === "dark" ? 0.5 : 0.2}
              zIndex={1 /** to not overlay topbar */}
              {...props}
            />
          )}
        >
          <Text fw="bold" fs="italic" c="dimmed">
            {isDisabled ? "Disabled" : "Unreachable"}
          </Text>
        </Center>
      )}
    </Stack>
  );
}

function StatItem({
  icon: Icon,
  label,
  percentage,
  type,
  isUnreachable,
  intention,
}: {
  icon: LucideIcon;
  label: string;
  percentage: number;
  type: "cpu" | "memory" | "disk";
  isUnreachable: boolean;
  intention: (
    percentage: number,
    type: "cpu" | "memory" | "disk",
  ) => ColorIntention;
}) {
  return (
    <Group gap="xs" wrap="nowrap" c="dimmed">
      <Icon size={13} />
      <Stack gap="0.1rem" w="calc(100% - 20px)">
        <Group justify="space-between">
          <Text size="sm">{label}</Text>
          <Text
            size="sm"
            c={
              isUnreachable
                ? "dimmed"
                : hexColorByIntention(intention(percentage, type))
            }
          >
            {isUnreachable ? "N/A" : `${percentage.toFixed(1)}%`}
          </Text>
        </Group>
        <Progress color="bw" value={percentage} size={4} />
      </Stack>
    </Group>
  );
}
