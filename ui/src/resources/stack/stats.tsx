import { ReactNode } from "react";
import { Select, SimpleGrid, Text } from "@mantine/core";
import { Types } from "komodo_client";
import { Section } from "mogh_ui";
import { useRead } from "@/lib/hooks";
import { ICONS } from "@/lib/icons";
import { useStatsGranularity } from "@/resources/server/hooks";
import {
  ChartRecord,
  ChartSeries,
  historicalRefetchInterval,
  SERIES_COLORS,
  SeriesChart,
} from "@/resources/server/stats/series-chart";
import { useStack } from ".";

/** The stack's CPU and memory over time, across every agent it ran on. */
export default function StackStats({
  id,
  titleOther,
}: {
  id: string;
  titleOther?: ReactNode;
}) {
  const [granularity, setGranularity] = useStatsGranularity();
  const name = useStack(id)?.name ?? id;
  const { data, isPending } = useRead(
    "GetHistoricalStackStats",
    { stacks: [id], granularity },
    { refetchInterval: historicalRefetchInterval(granularity) },
  );
  const series: ChartSeries[] = [
    {
      name,
      color: SERIES_COLORS[0],
      stats: data?.stats ?? [],
      usage: (record: ChartRecord) =>
        record.stacks?.find((stack) => stack.id === id),
    },
  ];
  return (
    <Section
      titleOther={titleOther}
      titleRight={
        <Select
          value={granularity}
          onChange={(granularity) =>
            granularity && setGranularity(granularity as Types.Timelength)
          }
          data={[
            Types.Timelength.FiveSeconds,
            Types.Timelength.FifteenSeconds,
            Types.Timelength.ThirtySeconds,
            Types.Timelength.OneMinute,
            Types.Timelength.FiveMinutes,
            Types.Timelength.FifteenMinutes,
            Types.Timelength.ThirtyMinutes,
            Types.Timelength.OneHour,
            Types.Timelength.SixHours,
            Types.Timelength.OneDay,
          ]}
          w={120}
        />
      }
      mb="md"
    >
      <Text c="dimmed" size="sm">
        Recorded wherever the stack ran, so its history stays after a move to
        another Server.
      </Text>
      <SimpleGrid cols={{ base: 1, xl: 2 }} spacing="xl">
        <SeriesChart
          kind="Cpu Cores"
          icon={<ICONS.Cpu size="1.1rem" />}
          note="cores"
          series={series}
          isPending={isPending}
        />
        <SeriesChart
          kind="Memory Bytes"
          icon={<ICONS.Memory size="1.1rem" />}
          note="working set"
          series={series}
          isPending={isPending}
        />
      </SimpleGrid>
    </Section>
  );
}
