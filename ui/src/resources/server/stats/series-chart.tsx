import { fmtBytes, fmtCores } from "@/components/usage";
import { komodo_client } from "@/lib/hooks";
import { Center, Group, Loader, Stack, Text } from "@mantine/core";
import { Types } from "komodo_client";
import { fmtSizeBytes } from "mogh_ui";
import { ReactNode, useMemo } from "react";
import {
  CartesianGrid,
  Legend,
  Line,
  LineChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";

/**
 * Fixed categorical palette for chart series (Mantine hues,
 * CVD-separation and contrast validated on light and dark surfaces).
 */
export const SERIES_COLORS = [
  "#1c7ed6", // blue.7
  "#e8590c", // orange.8
  "#0ca678", // teal.7
  "#be4bdb", // grape.6
  "#f03e3e", // red.7
  "#1098ad", // cyan.7
  "#7950f2", // violet.6
  "#d6336c", // pink.7
];

/** Beyond this many series, lines become unreadable and colors would repeat. */
export const MAX_SERIES = SERIES_COLORS.length;

/** A neutral color for a total drawn alongside its parts. */
export const TOTAL_COLOR = "#adb5bd"; // gray.5

/** Host stats, plus container usage in cores and bytes. */
export type ChartKind =
  | "Cpu"
  | "Memory"
  | "Disk"
  | "Load Average"
  | "Network Ingress"
  | "Network Egress"
  | "Cpu Cores"
  | "Memory Bytes";

const seriesKey = (i: number) => `series-${i}`;

export interface ChartSeries {
  name: string;
  color: string;
  stats: Types.SystemStatsRecord[];
  /** Where "Cpu Cores" and "Memory Bytes" read a record's usage from.
   * Default: the agent's total over all its containers. */
  usage?: (
    record: Types.SystemStatsRecord,
  ) => { cpu_cores?: number; mem_bytes?: number } | undefined;
  dashed?: boolean;
}

export function historicalRefetchInterval(granularity: Types.Timelength) {
  return granularity === Types.Timelength.FiveSeconds
    ? 5_000
    : granularity === Types.Timelength.FifteenSeconds
      ? 10_000
      : 15_000;
}

/** The same query `useRead("GetHistoricalServerStats", ...)` makes,
 * for `useQueries`, so the two share a cache entry. */
export function historicalStatsQuery(
  server: string,
  granularity: Types.Timelength,
) {
  return {
    queryKey: ["GetHistoricalServerStats", { server, granularity }],
    queryFn: () =>
      komodo_client().read("GetHistoricalServerStats", {
        server,
        granularity,
      }),
    refetchInterval: historicalRefetchInterval(granularity),
  };
}

/** One line per series. Records merge across series by timestamp. */
export function SeriesChart({
  kind,
  icon,
  note,
  series,
  isPending,
}: {
  kind: ChartKind;
  icon: ReactNode;
  /** Dimmed text after the title, eg. "host" or "cores". */
  note?: ReactNode;
  series: ChartSeries[];
  isPending: boolean;
}) {
  // Timestamps are bucket-aligned to the granularity server side,
  // so records merge across servers by exact ts.
  const chartData = useMemo(() => {
    const byTs = new Map<number, Record<string, number>>();
    series.forEach((s, i) => {
      for (const record of s.stats) {
        const entry = byTs.get(record.ts) ?? { date: record.ts };
        const value = getValue(record, kind, s);
        // A missing reading leaves a gap, never a 0. Keyed by position,
        // not name: a series named "date" (or two with one name) would
        // overwrite another key.
        if (value !== undefined) entry[seriesKey(i)] = value;
        byTs.set(record.ts, entry);
      }
    });
    return [...byTs.values()].sort((a, b) => a.date - b.date);
  }, [series, kind]);

  const minTime = chartData[0]?.date ?? 0;
  const maxTime = chartData[chartData.length - 1]?.date ?? 0;
  const timeDiff = maxTime - minTime;

  const isNetwork = kind === "Network Ingress" || kind === "Network Egress";
  const isLoadAvg = kind === "Load Average";
  const isCores = kind === "Cpu Cores";
  const isBytes = kind === "Memory Bytes";
  const isPercent = !isNetwork && !isLoadAvg && !isCores && !isBytes;

  const fmtValue = (value: number, precise: boolean) => {
    if (isNetwork) return fmtSizeBytes(value);
    if (isLoadAvg) return value.toFixed(precise ? 2 : 1);
    // Idle containers use thousandths of a core: two decimals would
    // label every tick 0.00.
    if (isCores)
      return value === 0 || value >= 0.1
        ? fmtCores(value, precise)
        : `${value.toFixed(3)}${precise ? " cores" : ""}`;
    if (isBytes) return fmtBytes(value);
    return `${value.toFixed(precise ? 1 : 0)}%`;
  };

  const xTickFormatter = (ts: number) => {
    const date = new Date(ts);
    if (timeDiff < 2 * 60 * 60 * 1000) {
      return date.toLocaleTimeString([], {
        hour: "2-digit",
        minute: "2-digit",
      });
    }
    return date.toLocaleString([], {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  };

  return (
    <Stack gap={4} className="bordered-light" p="md" bdrs="md">
      <Group gap="xs">
        {icon}
        <Text fz="xl" fw={500}>
          {chartTitle(kind)}
        </Text>
        {note && (
          <Text c="dimmed" size="sm">
            {note}
          </Text>
        )}
      </Group>
      {isPending ? (
        <Center h={200}>
          <Loader size="xl" />
        </Center>
      ) : (
        <ResponsiveContainer width="100%" height={200}>
          <LineChart
            data={chartData}
            margin={{ top: 5, right: 5, bottom: 0, left: 0 }}
          >
            <CartesianGrid
              strokeDasharray="3 3"
              stroke="rgba(255,255,255,0.07)"
              vertical={false}
            />
            <XAxis
              dataKey="date"
              type="number"
              domain={["dataMin", "dataMax"]}
              tickFormatter={xTickFormatter}
              tick={{ fontSize: 10, fill: "var(--mantine-color-dimmed)" }}
              axisLine={false}
              tickLine={false}
              minTickGap={60}
            />
            <YAxis
              tickFormatter={(value: number) => fmtValue(value, false)}
              tick={{ fontSize: 10, fill: "var(--mantine-color-dimmed)" }}
              axisLine={false}
              tickLine={false}
              width={isNetwork || isBytes ? 70 : 42}
              domain={
                isPercent
                  ? [0, 100]
                  : isCores || isBytes
                    ? [0, "auto"]
                    : ["auto", "auto"]
              }
            />
            <Legend
              iconType="line"
              wrapperStyle={{ fontSize: 11, paddingTop: 4 }}
            />
            <Tooltip
              contentStyle={{
                backgroundColor: "var(--mantine-color-dark-7)",
                border: "1px solid var(--mantine-color-dark-4)",
                borderRadius: "var(--mantine-radius-sm)",
                fontSize: 12,
              }}
              labelStyle={{ color: "var(--mantine-color-dimmed)" }}
              labelFormatter={(label) =>
                new Date(Number(label)).toLocaleString()
              }
              // List the busiest series first.
              itemSorter={(item) => -Number(item.value)}
              formatter={(value, name) => [fmtValue(Number(value), true), name]}
            />
            {series.map(({ name, color, dashed }, i) =>
              // A series with nothing in this window (an agent offline
              // for longer) gets no line and no legend entry.
              !chartData.some((entry) => seriesKey(i) in entry) ? null : (
                <Line
                  key={seriesKey(i)}
                  type="monotone"
                  dataKey={seriesKey(i)}
                  name={name}
                  stroke={color}
                  strokeWidth={2}
                  strokeDasharray={dashed ? "5 4" : undefined}
                  dot={false}
                  activeDot={{ r: 3 }}
                  connectNulls
                  isAnimationActive={false}
                />
              ),
            )}
          </LineChart>
        </ResponsiveContainer>
      )}
    </Stack>
  );
}

function chartTitle(kind: ChartKind) {
  switch (kind) {
    case "Load Average":
      return "Load Average (1m)";
    case "Cpu Cores":
      return "Cpu";
    case "Memory Bytes":
      return "Memory";
    default:
      return kind;
  }
}

function getValue(
  stat: Types.SystemStatsRecord,
  kind: ChartKind,
  series: ChartSeries,
): number | undefined {
  switch (kind) {
    case "Cpu":
      return stat.cpu_perc || 0;
    case "Memory":
      return stat.mem_total_gb > 0
        ? (100 * stat.mem_used_gb) / stat.mem_total_gb
        : 0;
    case "Disk":
      return stat.disk_total_gb > 0
        ? (100 * stat.disk_used_gb) / stat.disk_total_gb
        : 0;
    case "Network Ingress":
      return stat.network_ingress_bytes || 0;
    case "Network Egress":
      return stat.network_egress_bytes || 0;
    case "Load Average":
      return stat.load_average?.one ?? 0;
    case "Cpu Cores":
      return (series.usage ?? agentUsage)(stat)?.cpu_cores;
    case "Memory Bytes":
      return (series.usage ?? agentUsage)(stat)?.mem_bytes;
  }
}

function agentUsage(stat: Types.SystemStatsRecord) {
  return stat.containers;
}
