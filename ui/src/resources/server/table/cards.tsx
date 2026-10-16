import { useDashboardPreferences, useRead } from "@/lib/hooks";
import { ICONS } from "@/lib/icons";
import { usableResourcePath } from "@/lib/utils";
import {
  BoxProps,
  Card,
  Group,
  Progress,
  SimpleGrid,
  Stack,
  Text,
} from "@mantine/core";
import { Types } from "komodo_client";
import { StatusBadge, hexColorByIntention } from "mogh_ui";
import { useMemo } from "react";
import { Link } from "react-router-dom";
import AgentBadge from "../agent-badge";
import {
  ServerHost,
  diskPerc,
  groupServersByHost,
  hostIntention,
  hostStackCount,
  memPerc,
  useStacksByServer,
} from "../hosts";
import { serverThresholds } from "../hooks";

export default function HostServerCards({
  resources,
  ...boxProps
}: {
  resources: Types.ServerListItem[];
} & BoxProps) {
  const hosts = useMemo(() => groupServersByHost(resources), [resources]);
  return (
    <SimpleGrid
      cols={{ base: 1, sm: 2, lg: 3, xl: 4 }}
      spacing="md"
      {...boxProps}
    >
      {hosts.map((host) => (
        <HostCard key={host.name} host={host} />
      ))}
    </SimpleGrid>
  );
}

function HostCard({ host }: { host: ServerHost }) {
  const { preferences } = useDashboardPreferences();
  const stacksByServer = useStacksByServer();
  const coreVersion = useRead("GetVersion", {}).data?.version;
  const stacks = hostStackCount(host, stacksByServer);
  const agents = host.servers.length;
  const intent = hostIntention(host, coreVersion);
  // A host with one agent links to that agent, otherwise to the root agent.
  const link =
    agents === 1
      ? host.servers[0]
      : host.servers.find((s) => !s.info.rootless_user);
  const version = host.mixedVersions
    ? "mixed versions"
    : (host.primary.info.version ?? "unknown version");

  return (
    <Card withBorder radius="md" padding="md">
      <Stack gap="sm">
        <Group justify="space-between" wrap="nowrap">
          <Group
            gap="xs"
            wrap="nowrap"
            renderRoot={
              link
                ? (props) => (
                    <Link
                      to={`/${usableResourcePath("Server")}/${link.id}`}
                      {...props}
                    />
                  )
                : undefined
            }
          >
            <ICONS.Server size="1rem" color={hexColorByIntention(intent)} />
            <Text
              fw="bold"
              className={link ? "hover-underline" : undefined}
              style={{ textWrap: "nowrap" }}
            >
              {host.name}
            </Text>
          </Group>
          <StatusBadge text={host.state} intent={intent} />
        </Group>

        <Text c="dimmed" size="sm">
          {stacks} stack{stacks === 1 ? "" : "s"} · {agents} agent
          {agents === 1 ? "" : "s"} · {version}
        </Text>

        {preferences.showServerStats && <HostStats server={host.primary} />}

        <Group gap="xs">
          {host.servers.map((server) => (
            <Link
              key={server.id}
              to={`/${usableResourcePath("Server")}/${server.id}`}
              style={{ display: "flex" }}
            >
              <AgentBadge
                server={server}
                count={stacksByServer.get(server.id)?.length ?? 0}
              />
            </Link>
          ))}
        </Group>
      </Stack>
    </Card>
  );
}

function HostStats({ server }: { server: Types.ServerListItem }) {
  const stats = server.info.stats;
  if (!stats) {
    return (
      <Text c="dimmed" size="sm">
        No stats
      </Text>
    );
  }
  const t = serverThresholds(server);
  return (
    <Stack gap={4}>
      <StatRow
        label="CPU"
        perc={stats.cpu_perc}
        warning={t.cpuWarning}
        critical={t.cpuCritical}
        text={`${stats.cpu_perc.toFixed(0)}%`}
      />
      <StatRow
        label="Mem"
        perc={memPerc(stats) ?? 0}
        warning={t.memWarning}
        critical={t.memCritical}
        text={`${stats.mem_used_gb.toFixed(1)} / ${stats.mem_total_gb.toFixed(1)} GB`}
      />
      <StatRow
        label="Disk"
        perc={diskPerc(stats) ?? 0}
        warning={t.diskWarning}
        critical={t.diskCritical}
        text={`${stats.disk_used_gb.toFixed(0)} / ${stats.disk_total_gb.toFixed(0)} GB`}
      />
    </Stack>
  );
}

function StatRow({
  label,
  perc,
  warning,
  critical,
  text,
}: {
  label: string;
  perc: number;
  warning: number;
  critical: number;
  text: string;
}) {
  const intent =
    perc < warning ? "Good" : perc < critical ? "Warning" : "Critical";
  return (
    <Group gap="xs" wrap="nowrap">
      <Text c="dimmed" size="sm" w="2.5rem">
        {label}
      </Text>
      <Progress
        value={perc}
        color={hexColorByIntention(intent)}
        size="sm"
        w="6rem"
      />
      <Text c="dimmed" size="sm" style={{ textWrap: "nowrap" }}>
        {text}
      </Text>
    </Group>
  );
}
