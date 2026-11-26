import { useRead } from "@/lib/hooks";
import { ICONS } from "@/lib/icons";
import { useMemo } from "react";
import { useStatsGranularity } from "../hooks";
import {
  ChartRecord,
  ChartSeries,
  historicalRefetchInterval,
  MAX_SERIES,
  SERIES_COLORS,
  SeriesChart,
  TOTAL_COLOR,
} from "./series-chart";

/** One agent's own CPU and memory over time: a line per stack, plus
 * the agent's total over all its containers (dashed). Renders the two
 * charts as siblings, to sit in the caller's grid. */
export default function AgentUsageCharts({ id }: { id: string }) {
  const [granularity] = useStatsGranularity();
  const { data, isPending } = useRead(
    "GetHistoricalServerStats",
    { server: id, granularity },
    { refetchInterval: historicalRefetchInterval(granularity) },
  );
  const stackList = useRead("ListStacks", { limit: 0 }).data;

  const { series, charted, total } = useMemo(() => {
    const stats = data?.stats ?? [];
    const names = new Map((stackList ?? []).map((s) => [s.id, s.name]));
    // Rank stacks by their latest memory reading. Records come newest first.
    const latest = new Map<string, number>();
    const seen = new Set<string>();
    for (const record of stats) {
      for (const stack of record.stacks ?? []) {
        seen.add(stack.id);
        if (!latest.has(stack.id) && stack.mem_bytes !== undefined) {
          latest.set(stack.id, stack.mem_bytes);
        }
      }
    }
    // A stack with no memory reading at all ranks last.
    for (const stackId of seen) {
      if (!latest.has(stackId)) latest.set(stackId, -1);
    }
    const ranked = [...latest.entries()].sort((a, b) => b[1] - a[1]);
    // One color is left for the total.
    const charted = ranked.slice(0, MAX_SERIES - 1);
    const series: ChartSeries[] = [
      {
        name: "All containers",
        color: TOTAL_COLOR,
        stats,
        dashed: true,
      },
      ...charted.map(([stackId], i) => ({
        // A stack deleted since keeps its id as its name.
        name: names.get(stackId) ?? stackId,
        color: SERIES_COLORS[i],
        stats,
        usage: (record: ChartRecord) =>
          record.stacks?.find((s) => s.id === stackId),
      })),
    ];
    return { series, charted: charted.length, total: ranked.length };
  }, [data, stackList]);

  const note = (unit: string) =>
    total > charted
      ? `${unit} · this agent · top ${charted} of ${total} stacks`
      : `${unit} · this agent`;

  return (
    <>
      <SeriesChart
        kind="Cpu Cores"
        icon={<ICONS.Cpu size="1.1rem" />}
        note={note("cores")}
        series={series}
        isPending={isPending}
      />
      <SeriesChart
        kind="Memory Bytes"
        icon={<ICONS.Memory size="1.1rem" />}
        note={note("working set")}
        series={series}
        isPending={isPending}
      />
    </>
  );
}
