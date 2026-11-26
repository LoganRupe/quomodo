import { Group, Progress, Text, TextProps, Tooltip } from "@mantine/core";
import { Types } from "komodo_client";

/** Container CPU and memory, as the agent measures it from cgroup v2
 * files. A number that is missing is shown as a dash, never 0. */

const KiB = 1024;
const MiB = 1024 * KiB;
const GiB = 1024 * MiB;

export const USAGE_MISSING =
  "Not measured. The container just started (CPU needs two samples), " +
  "the host has no cgroup v2, stats monitoring is off for this server, " +
  "or its agent is older than this feature.";

export function fmtCores(cores: number | undefined, unit = false) {
  if (cores === undefined || !Number.isFinite(cores)) return "–";
  const n = cores >= 10 ? cores.toFixed(1) : cores.toFixed(2);
  return unit ? `${n} cores` : n;
}

export function fmtBytes(bytes: number | undefined) {
  if (bytes === undefined || !Number.isFinite(bytes)) return "–";
  if (bytes >= GiB) return `${(bytes / GiB).toFixed(2)} GiB`;
  if (bytes >= MiB) return `${Math.round(bytes / MiB)} MiB`;
  return `${Math.round(bytes / KiB)} KiB`;
}

export function fmtUsage(usage: Types.ContainerUsage | undefined) {
  return `${fmtCores(usage?.cpu_cores, true)} · ${fmtBytes(usage?.mem_bytes)}`;
}

/** The size of the host a server's agent runs on, to show usage as a
 * share of it. Every agent on a machine reports the same host. */
export function hostSize(server: Types.ServerListItem | undefined) {
  const cores = server?.info.logical_core_count;
  const memGb = server?.info.stats?.mem_total_gb;
  return {
    cores: cores && cores > 0 ? cores : undefined,
    memBytes: memGb && memGb > 0 ? memGb * GiB : undefined,
  };
}

export type UsageKind = "cpu" | "mem";

export function usageValue(
  usage: Types.ContainerUsage | undefined,
  kind: UsageKind,
) {
  return kind === "cpu" ? usage?.cpu_cores : usage?.mem_bytes;
}

/** Sort key for a usage column: missing numbers sort last. */
export function usageSortValue(
  usage: Types.ContainerUsage | undefined,
  kind: UsageKind,
) {
  return usageValue(usage, kind) ?? -1;
}

/** A usage number in a table cell. A missing one is a dash with a
 * tooltip saying why. With `of`, a small green bar shows its share
 * of the host (cores, or bytes of memory). */
export function UsageCell({
  usage,
  kind,
  of,
  unit,
  showLimit,
  textProps,
}: {
  usage: Types.ContainerUsage | undefined;
  kind: UsageKind;
  /** The host's size in the same unit, for the bar. */
  of?: number;
  /** Write "cores" after a CPU number. */
  unit?: boolean;
  /** Show " / limit" after memory when the unit sets one. */
  showLimit?: boolean;
  textProps?: TextProps;
}) {
  const value = usageValue(usage, kind);
  if (value === undefined) {
    return (
      <Tooltip label={USAGE_MISSING} w={320} multiline>
        <Text c="dimmed" ff="monospace" size="sm" {...textProps}>
          –
        </Text>
      </Tooltip>
    );
  }
  const text = kind === "cpu" ? fmtCores(value, unit) : fmtBytes(value);
  const limit =
    kind === "mem" && showLimit ? usage?.mem_limit_bytes : undefined;
  const share = of ? Math.min(100, (value / of) * 100) : undefined;
  return (
    <Group gap="xs" wrap="nowrap">
      <Text
        ff="monospace"
        size="sm"
        style={{ whiteSpace: "nowrap" }}
        {...textProps}
      >
        {text}
        {limit !== undefined && (
          <Text span c="dimmed" ff="monospace" size="sm">
            {" "}
            / {fmtBytes(limit)}
          </Text>
        )}
      </Text>
      {share !== undefined && (
        <Tooltip
          label={
            kind === "cpu"
              ? `${fmtCores(value)} of ${of} cores on the host`
              : `${fmtBytes(value)} of ${fmtBytes(of)} on the host`
          }
        >
          <Progress
            color="green"
            value={share}
            size={6}
            w={64}
            style={{ flexShrink: 0 }}
          />
        </Tooltip>
      )}
    </Group>
  );
}
