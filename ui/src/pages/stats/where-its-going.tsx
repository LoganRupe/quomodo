import { fmtBytes, hostSize, UsageCell } from "@/components/usage";
import ResourceLink from "@/resources/link";
import { agentLabel, ServerHost } from "@/resources/server/hosts";
import { SERIES_COLORS } from "@/resources/server/stats/series-chart";
import { Box, Group, Progress, Stack, Text, Tooltip } from "@mantine/core";
import { Types } from "komodo_client";
import { ChartPie } from "lucide-react";
import { DataTable, Section } from "mogh_ui";
import { useMemo } from "react";

const GiB = 1024 * 1024 * 1024;

/** Stacks named in the bar; the rest share one "more stacks" segment. */
const BAR_STACKS = 4;
/** Rows in the top stacks table. */
const TABLE_STACKS = 10;

const MORE_COLOR = "#868e96"; // gray.6
const OTHER_COLOR = "#495057"; // gray.7
const OUTSIDE_COLOR = "#5f636a";

interface StackRow {
  stack: Types.StackListItem;
  server: Types.ServerListItem;
  host: ServerHost;
}

interface Segment {
  label: string;
  bytes: number;
  color: string;
  tooltip?: string;
}

/** Live memory of the selected hosts, split by stack. With one host,
 * a bar shows how its used memory divides between stacks, other
 * containers and everything outside containers. */
export default function WhereItsGoing({
  hosts,
  stacks,
}: {
  hosts: ServerHost[];
  stacks: Types.StackListItem[];
}) {
  const rows = useMemo(() => {
    const byServer = new Map<
      string,
      { server: Types.ServerListItem; host: ServerHost }
    >();
    for (const host of hosts) {
      for (const server of host.servers) {
        byServer.set(server.id, { server, host });
      }
    }
    const rows: StackRow[] = [];
    for (const stack of stacks) {
      const usage = stack.info.usage;
      if (usage?.mem_bytes === undefined && usage?.cpu_cores === undefined) {
        continue;
      }
      const found = byServer.get(stack.info.server_id);
      if (found) rows.push({ stack, ...found });
    }
    // Most memory first; a stack with no memory reading goes last.
    return rows.sort(
      (a, b) =>
        (b.stack.info.usage?.mem_bytes ?? -1) -
        (a.stack.info.usage?.mem_bytes ?? -1),
    );
  }, [hosts, stacks]);

  if (hosts.length === 0) return null;

  const single = hosts.length === 1 ? hosts[0] : undefined;

  let used = 0;
  let total = 0;
  for (const host of hosts) {
    used += (host.primary.info.stats?.mem_used_gb ?? 0) * GiB;
    total += (host.primary.info.stats?.mem_total_gb ?? 0) * GiB;
  }

  return (
    <Section
      withBorder
      title="Where it's going"
      icon={<ChartPie size="1.3rem" />}
      titleRight={
        <Group ml={{ sm: "xl" }} gap="md">
          <Text c="dimmed" size="sm">
            {single ? single.name : `${hosts.length} hosts`} · live
          </Text>
          {total > 0 && (
            <Text c="dimmed" size="sm" ff="monospace">
              {fmtBytes(used)} of {fmtBytes(total)}
            </Text>
          )}
        </Group>
      }
    >
      <Stack>
        {single && total > 0 && (
          <HostBar host={single} rows={rows} used={used} total={total} />
        )}
        {rows.length === 0 ? (
          <Text c="dimmed" size="sm">
            No stack usage measured on{" "}
            {single ? "this host" : "the selected hosts"} yet.
          </Text>
        ) : (
          <>
            {!single && (
              <Text c="dimmed" size="sm">
                Top {Math.min(TABLE_STACKS, rows.length)} of {rows.length}{" "}
                stacks across {hosts.length} hosts, by memory. Filter to one
                host to see how its memory splits.
              </Text>
            )}
            <DataTable
              tableKey="stats-top-stacks"
              data={rows.slice(0, TABLE_STACKS)}
              getRowId={(row) => row.stack.id}
              columns={[
                {
                  header: "Stack",
                  cell: ({ row }) => (
                    <ResourceLink type="Stack" id={row.original.stack.id} />
                  ),
                },
                {
                  header: single ? "Agent" : "Host",
                  cell: ({ row }) => (
                    <Text size="sm" c="dimmed">
                      {single
                        ? agentLabel(row.original.server)
                        : `${row.original.host.name} · ${agentLabel(row.original.server)}`}
                    </Text>
                  ),
                },
                {
                  header: "CPU",
                  cell: ({ row }) => (
                    <UsageCell
                      usage={row.original.stack.info.usage}
                      kind="cpu"
                      unit
                      of={hostSize(row.original.host.primary).cores}
                    />
                  ),
                },
                {
                  header: "Memory",
                  cell: ({ row }) => (
                    <UsageCell
                      usage={row.original.stack.info.usage}
                      kind="mem"
                      of={hostSize(row.original.host.primary).memBytes}
                    />
                  ),
                },
              ]}
            />
          </>
        )}
      </Stack>
    </Section>
  );
}

function HostBar({
  host,
  rows,
  used,
  total,
}: {
  host: ServerHost;
  rows: StackRow[];
  used: number;
  total: number;
}) {
  const measured = rows.filter(
    (row) => row.stack.info.usage?.mem_bytes !== undefined,
  );
  const stackBytes = (row: StackRow) => row.stack.info.usage?.mem_bytes ?? 0;

  const segments: Segment[] = measured.slice(0, BAR_STACKS).map((row, i) => ({
    label: row.stack.name,
    bytes: stackBytes(row),
    color: SERIES_COLORS[i],
  }));

  const rest = measured.slice(BAR_STACKS);
  if (rest.length) {
    segments.push({
      label: `${rest.length} more stack${rest.length === 1 ? "" : "s"}`,
      bytes: rest.reduce((sum, row) => sum + stackBytes(row), 0),
      color: MORE_COLOR,
    });
  }
  const stacksTotal = measured.reduce((sum, row) => sum + stackBytes(row), 0);

  // The agents' totals cover every container, in a stack or not. Only
  // when every reachable agent has one is the remainder known; a partial
  // sum would pass part of the containers off as "outside containers".
  const reachable = host.servers.filter(
    (s) => s.info.state === Types.ServerState.Ok,
  );
  const complete =
    reachable.length > 0 &&
    reachable.every((s) => s.info.usage?.mem_bytes !== undefined);
  if (complete) {
    const containers = reachable.reduce(
      (sum, s) => sum + (s.info.usage?.mem_bytes ?? 0),
      0,
    );
    const other = containers - stacksTotal;
    // Ignore rounding noise between the stack and agent roll ups.
    if (other > 16 * 1024 * 1024) {
      segments.push({
        label: "other containers",
        bytes: other,
        color: OTHER_COLOR,
        tooltip:
          "Containers outside these stacks, or in a stack without a full reading.",
      });
    }
    const outside = used - Math.max(containers, stacksTotal);
    if (outside > 0) {
      segments.push({
        label: "outside containers",
        bytes: outside,
        color: OUTSIDE_COLOR,
        tooltip: "Host memory in use by anything not in a container.",
      });
    }
  }

  return (
    <Stack gap="xs">
      <Progress.Root size={16}>
        {segments.map((segment) => (
          <Tooltip
            key={segment.label}
            label={`${segment.label}: ${fmtBytes(segment.bytes)}`}
          >
            <Progress.Section
              value={Math.min(100, (segment.bytes / total) * 100)}
              color={segment.color}
            />
          </Tooltip>
        ))}
      </Progress.Root>
      <Group gap="md">
        {segments.map((segment) => (
          <Tooltip
            key={segment.label}
            label={segment.tooltip}
            disabled={!segment.tooltip}
            w={280}
            multiline
          >
            <Group gap={6} wrap="nowrap">
              <Box
                w={10}
                h={10}
                bg={segment.color}
                style={{ borderRadius: 2, flexShrink: 0 }}
              />
              <Text size="sm">
                {segment.label}{" "}
                <Text span c="dimmed" size="sm" ff="monospace">
                  {fmtBytes(segment.bytes)}
                </Text>
              </Text>
            </Group>
          </Tooltip>
        ))}
      </Group>
    </Stack>
  );
}
