import { useMemo } from "react";
import { Group, Text, Tooltip } from "@mantine/core";
import { Types } from "komodo_client";
import { useRead } from "@/lib/hooks";
import { ServerHost, groupServersByHost } from "@/resources/server/hosts";
import {
  UsageKind,
  fmtBytes,
  fmtCores,
  hostSize,
  usageValue,
} from "@/components/usage";

const GiB = 1024 * 1024 * 1024;

/** Stack states where some container may be running, so a missing
 * usage reading means a missing number rather than nothing to count. */
const RUNNING_STACK_STATES = new Set<Types.StackState>([
  Types.StackState.Running,
  Types.StackState.Unhealthy,
  Types.StackState.Restarting,
  Types.StackState.Paused,
  Types.StackState.Deploying,
]);

/** Every Server grouped by host, and each Server's host by its id. */
export function useServerHosts() {
  const servers = useRead("ListServers", { limit: 0 }).data;
  return useMemo(() => {
    const hosts = groupServersByHost(servers ?? []);
    const byServer = new Map<string, ServerHost>();
    for (const host of hosts) {
      for (const server of host.servers) byServer.set(server.id, host);
    }
    return { hosts, byServer };
  }, [servers]);
}

/** The size of a host, read from the agent that reports its stats. */
export function serverHostSize(host: ServerHost | undefined) {
  return hostSize(host?.primary);
}

/** The sum of the stacks' usage. Undefined when a stack that may have
 * running containers has no reading, so a partial sum never poses as
 * the whole. */
export function sumStackUsage(
  stacks: Types.StackListItem[],
  kind: UsageKind,
): number | undefined {
  let sum = 0;
  for (const stack of stacks) {
    const value = usageValue(stack.info.usage, kind);
    if (value === undefined) {
      if (RUNNING_STACK_STATES.has(stack.info.state)) return undefined;
      continue;
    }
    sum += value;
  }
  return sum;
}

/** A host's summed usage against its size, eg "0.39 of 4 cores". */
export function HostUsageCell({
  value,
  kind,
  of,
}: {
  value: number | undefined;
  kind: UsageKind;
  /** The host's size: cores, or bytes of memory. */
  of: number | undefined;
}) {
  if (value === undefined) {
    return (
      <Tooltip
        label="A running stack on this host has no reading, so the total is left blank."
        w={320}
        multiline
      >
        <Text c="dimmed" ff="monospace" size="sm">
          –
        </Text>
      </Tooltip>
    );
  }
  let text: string;
  let size: string | undefined;
  if (kind === "cpu") {
    text = fmtCores(value, !of);
    size = of ? `of ${of} cores` : undefined;
  } else if (of) {
    text = (value / GiB).toFixed(2);
    const total = of / GiB;
    size = `of ${Number.isInteger(total) ? total : total.toFixed(1)} GiB`;
  } else {
    text = fmtBytes(value);
  }
  return (
    <Group gap={6} wrap="nowrap">
      <Text ff="monospace" size="sm" style={{ whiteSpace: "nowrap" }}>
        {text}
      </Text>
      {size && (
        <Text
          c="dimmed"
          ff="monospace"
          size="sm"
          style={{ whiteSpace: "nowrap" }}
        >
          {size}
        </Text>
      )}
    </Group>
  );
}
