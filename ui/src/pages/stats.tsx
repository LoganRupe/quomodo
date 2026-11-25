import TagsFilter from "@/components/tags/filter";
import { fmtBytes } from "@/components/usage";
import {
  useDebouncedTermSearch,
  useRead,
  useSetTitle,
  useTagsFilter,
} from "@/lib/hooks";
import { ICONS } from "@/lib/icons";
import ResourceMultiSelector from "@/resources/multi-selector";
import { useStatsGranularity } from "@/resources/server/hooks";
import { groupServersByHost, ServerHost } from "@/resources/server/hosts";
import {
  ChartKind,
  ChartSeries,
  historicalStatsQuery,
  MAX_SERIES,
  SeriesChart,
  SERIES_COLORS,
} from "@/resources/server/stats/series-chart";
import StatsServerTable from "@/resources/server/table/stats";
import {
  Group,
  Progress,
  SegmentedControl,
  Select,
  SimpleGrid,
  Stack,
  Text,
  Tooltip,
} from "@mantine/core";
import { useLocalStorage } from "@mantine/hooks";
import { keepPreviousData, useQueries } from "@tanstack/react-query";
import { Types } from "komodo_client";
import { ChartLine, Download, Upload } from "lucide-react";
import {
  fmtUpperCamelcase,
  hexColorByIntention,
  InfoCard,
  Page,
  SearchInput,
  Section,
  StatBar,
} from "mogh_ui";
import { ReactNode, useMemo } from "react";
import WhereItsGoing from "./stats/where-its-going";

/** History is fetched for this many candidates, to fill MAX_SERIES
 * lines with ones that have readings. */
const CANDIDATES = 2 * MAX_SERIES;

const GiB = 1024 * 1024 * 1024;

const HOST_CHARTS: [ChartKind, ReactNode][] = [
  ["Load Average", <ICONS.LoadAvg size="1.1rem" />],
  ["Cpu", <ICONS.Cpu size="1.1rem" />],
  ["Memory", <ICONS.Memory size="1.1rem" />],
  ["Disk", <ICONS.Disk size="1.1rem" />],
  ["Network Ingress", <Download size="1.1rem" />],
  ["Network Egress", <Upload size="1.1rem" />],
];

type HistoricalView = "hosts" | "agents" | "stacks";

const HISTORICAL_VIEWS: { value: HistoricalView; label: string }[] = [
  { value: "hosts", label: "Hosts" },
  { value: "agents", label: "Agents" },
  { value: "stacks", label: "Stacks" },
];

export default function Stats() {
  useSetTitle("Stats");

  // Server names
  const [selectedServers, setSelectedServers] = useLocalStorage<string[]>({
    key: "stats.selected-servers.v1",
    defaultValue: [],
  });

  const { search, setSearch, terms } = useDebouncedTermSearch({
    localStorageKey: "stats.search.v1",
  });
  const tags = useTagsFilter();
  // A search or tag filter can hide some of a host's agents, so their
  // containers are missing from any per-host container total.
  const filtered = terms.length > 0 || tags.length > 0;

  // Server side sort, passed up from the table.
  const [sortBy, setSortBy] = useLocalStorage<Types.ServerSortBy>({
    key: "stats.sort.v1",
    defaultValue: Types.ServerSortBy.LoadAverage,
  });

  const serversData = useRead(
    "ListServers",
    {
      query: { terms, tags },
      limit: 0,
      sort_by: sortBy,
      sort_desc: true,
    },
    { refetchInterval: 30_000, placeholderData: keepPreviousData },
  ).data;
  const servers = useMemo(() => serversData ?? [], [serversData]);

  // Same cadence as the servers, so the breakdown never pairs fresh
  // agent totals with old stack usage.
  const stacksData = useRead(
    "ListStacks",
    { limit: 0 },
    { refetchInterval: 30_000 },
  ).data;
  const stacks = useMemo(() => stacksData ?? [], [stacksData]);

  const filteredServers = useMemo(
    () =>
      selectedServers.length
        ? servers.filter((server) => selectedServers.includes(server.name))
        : servers,
    [servers, selectedServers],
  );

  // Every agent on one machine reports the same host stats, so the
  // summary, breakdown and host charts count each host once. Selecting
  // any agent of a host selects the whole host.
  const allHosts = useMemo(() => groupServersByHost(servers), [servers]);
  const hosts = useMemo(() => {
    // In the server side sort order: a host sorts where its first agent does.
    const position = new Map(servers.map((s, i) => [s.id, i]));
    const first = (host: ServerHost) =>
      Math.min(...host.servers.map((s) => position.get(s.id) ?? Infinity));
    return allHosts
      .filter(
        (host) =>
          !selectedServers.length ||
          host.servers.some((s) => selectedServers.includes(s.name)),
      )
      .sort((a, b) => first(a) - first(b));
  }, [servers, allHosts, selectedServers]);

  // Stable color per host, keyed on the full (unfiltered, name sorted)
  // list so the assignment survives filtering and sorting.
  const hostColors = useMemo(() => {
    const colors = new Map<string, string>();
    allHosts.forEach((host, i) =>
      colors.set(host.name, SERIES_COLORS[i % SERIES_COLORS.length]),
    );
    return colors;
  }, [allHosts]);

  const Content =
    servers.length === 0 ? (
      <Text c="dimmed">No Servers found.</Text>
    ) : (
      <>
        <CurrentStatsSummary hosts={hosts} filtered={filtered} />
        <WhereItsGoing hosts={hosts} stacks={stacks} filtered={filtered} />
        <HistoricalStats
          hosts={hosts}
          hostColors={hostColors}
          stacks={stacks}
        />
        <Section title="Servers" icon={<ICONS.Server size="1.3rem" />}>
          <StatsServerTable resources={filteredServers} disableSorting />
        </Section>
      </>
    );

  return (
    <Page
      title="Stats"
      icon={ICONS.Stats}
      description="System stats across all servers."
    >
      <Stack>
        <Group justify="space-between">
          <Group w={{ base: "100%", xs: "fit-content" }}>
            <ResourceMultiSelector
              type="Server"
              value={selectedServers}
              onChange={setSelectedServers}
            />
            <Select
              value={sortBy}
              onChange={(s) => setSortBy(s ?? Types.ServerSortBy.LoadAverage)}
              data={[
                Types.ServerSortBy.LoadAverage,
                Types.ServerSortBy.Cpu,
                Types.ServerSortBy.Memory,
                Types.ServerSortBy.Disk,
                Types.ServerSortBy.Network,
              ].map((s) => ({ value: s, label: fmtUpperCamelcase(s) }))}
              allowDeselect={false}
              leftSection={
                <Text c="dimmed" size="sm" ml="2">
                  Sort By:
                </Text>
              }
              leftSectionWidth={64}
              leftSectionPointerEvents="none"
            />
          </Group>

          <Group w={{ base: "100%", xs: "fit-content" }}>
            <TagsFilter />
            <SearchInput value={search} onSearch={setSearch} />
          </Group>
        </Group>

        {Content}
      </Stack>
    </Page>
  );
}

function CurrentStatsSummary({
  hosts,
  filtered,
}: {
  hosts: ServerHost[];
  filtered: boolean;
}) {
  const agents = hosts.flatMap((host) => host.servers);
  const okCount = agents.filter(
    (server) => server.info.state === Types.ServerState.Ok,
  ).length;
  const disabledCount = agents.filter(
    (server) => server.info.state === Types.ServerState.Disabled,
  ).length;
  const unreachableCount = agents.length - okCount - disabledCount;

  // The list items already carry the latest stats,
  // no per server stats queries needed. One reading per host.
  const stats = hosts.flatMap((host) => host.primary.info.stats ?? []);
  const cpu =
    stats.length > 0
      ? stats.reduce((acc, s) => acc + s.cpu_perc, 0) / stats.length
      : 0;
  const memUsed = stats.reduce((acc, s) => acc + s.mem_used_gb, 0);
  const memTotal = stats.reduce((acc, s) => acc + s.mem_total_gb, 0);
  const diskUsed = stats.reduce((acc, s) => acc + s.disk_used_gb, 0);
  const diskTotal = stats.reduce((acc, s) => acc + s.disk_total_gb, 0);

  // Container memory: every agent's own total, summed over the hosts
  // that have stats (the same hosts memUsed covers).
  const statsAgents = hosts
    .filter((host) => host.primary.info.stats)
    .flatMap((host) => host.servers);
  const measuredAgents = statsAgents.filter(
    (s) => s.info.usage?.mem_bytes !== undefined,
  );
  const containersGb =
    measuredAgents.reduce((acc, s) => acc + (s.info.usage?.mem_bytes ?? 0), 0) /
    GiB;

  return (
    <Section>
      <Group align="stretch">
        <InfoCard
          title="Hosts"
          info={<ICONS.Server size="1.3rem" />}
          w={{ base: "100%", lg: 300 }}
          gap="0.2rem"
          justify="space-between"
        >
          <Text c="dimmed" size="sm">
            <b>{hosts.length}</b> host{hosts.length === 1 ? "" : "s"} ·{" "}
            {okCount === agents.length ? (
              <>
                <b>{okCount}</b> agent{okCount === 1 ? "" : "s"} reachable
              </>
            ) : (
              <>
                <b>{okCount}</b> of <b>{agents.length}</b> agents reachable
              </>
            )}
          </Text>
          <Group gap="md">
            {unreachableCount > 0 && (
              <Text c="red" size="sm">
                {unreachableCount} unreachable
              </Text>
            )}
            {disabledCount > 0 && (
              <Text c="dimmed" size="sm">
                {disabledCount} disabled
              </Text>
            )}
          </Group>
        </InfoCard>
        <StatBar
          title="CPU Usage"
          icon={<ICONS.Cpu size="1.3rem" />}
          description={
            <>
              Average across <b>{stats.length}</b> host
              {stats.length === 1 ? "" : "s"}
            </>
          }
          percentage={cpu}
          warning={75}
          critical={90}
        />
        <MemoryCard
          used={memUsed}
          total={memTotal}
          containers={measuredAgents.length ? containersGb : undefined}
          partial={
            filtered ||
            measuredAgents.length < statsAgents.length ||
            statsAgents.some((s) => s.info.state !== Types.ServerState.Ok)
          }
        />
        <StatBar
          title="Disk Usage"
          icon={<ICONS.Disk size="1.3rem" />}
          description={
            diskTotal > 0 && (
              <>
                <b>{diskUsed.toFixed(1)} GB</b> of{" "}
                <b>{diskTotal.toFixed(1)} GB</b> in use
              </>
            )
          }
          percentage={diskTotal > 0 ? (diskUsed / diskTotal) * 100 : 0}
          warning={75}
          critical={90}
        />
      </Group>
    </Section>
  );
}

/** StatBar for memory, with the bar split into containers (green)
 * and the rest of the used memory (the usual white). */
function MemoryCard({
  used,
  total,
  containers,
  partial,
}: {
  used: number;
  total: number;
  /** GB used by containers, undefined when no agent measured any. */
  containers: number | undefined;
  /** Some agents have no container reading. */
  partial: boolean;
}) {
  const percentage = total > 0 ? (used / total) * 100 : undefined;
  const intent =
    percentage === undefined
      ? undefined
      : percentage > 90
        ? "Critical"
        : percentage > 75
          ? "Warning"
          : "Good";
  const inContainers = Math.min(containers ?? 0, used);
  const containersPerc = total > 0 ? (inContainers / total) * 100 : 0;
  const restPerc = (percentage ?? 0) - containersPerc;
  return (
    <InfoCard
      title="Memory Usage"
      info={
        <Group gap="xs">
          <Text c={intent ? hexColorByIntention(intent) : "dimmed"} fz="lg">
            {percentage === undefined ? "N/A" : `${percentage.toFixed(2)}%`}
          </Text>
          <ICONS.Memory size="1.3rem" />
        </Group>
      }
      w={{ base: "100%", lg: 300 }}
      gap="0.2rem"
      justify="space-between"
    >
      {total > 0 && (
        <Text c="dimmed" size="sm">
          <b>{used.toFixed(1)} GB</b> of <b>{total.toFixed(1)} GB</b> in use
          {containers !== undefined && (
            <>
              , containers <b>{fmtBytes(containers * GiB)}</b>
              {partial && " (partial)"}
            </>
          )}
        </Text>
      )}
      <Progress.Root size="xl">
        {containers !== undefined && (
          <Tooltip
            label={`Containers: ${fmtBytes((containers ?? 0) * GiB)} measured${
              partial
                ? ", partial: an agent is unreachable, has no reading, or is hidden by the filter"
                : ""
            }`}
          >
            <Progress.Section value={containersPerc} color="green" />
          </Tooltip>
        )}
        <Tooltip
          label={
            containers === undefined
              ? `In use: ${fmtBytes(used * GiB)}`
              : `Rest of in use memory: ${fmtBytes((used - inContainers) * GiB)}${
                  partial
                    ? ", including containers on agents with no reading"
                    : ""
                }. Approximate: the host and containers count page cache differently.`
          }
        >
          <Progress.Section value={Math.max(0, restPerc)} color="bw" />
        </Tooltip>
      </Progress.Root>
    </InfoCard>
  );
}

function HistoricalStats({
  hosts,
  hostColors,
  stacks,
}: {
  hosts: ServerHost[];
  hostColors: Map<string, string>;
  stacks: Types.StackListItem[];
}) {
  const [granularity, setGranularity] = useStatsGranularity();
  const [view, setView] = useLocalStorage<HistoricalView>({
    key: "stats.historical-view.v1",
    defaultValue: "hosts",
  });

  // What each mode charts. Usage lines come from `usage` (agents, stacks),
  // host lines from each host's primary agent. Agents and stacks are
  // ranked by their live memory, so the top ones get a line.
  const plan = useMemo(() => {
    // Offline and disabled agents stay charted: their history is still
    // stored, and an outage is when it matters. One with nothing in the
    // window draws no line.
    const offline = (server: Types.ServerListItem) =>
      server.info.state !== Types.ServerState.Ok;
    // Live memory first, then unreachable (recent history), then
    // reachable with no reading, then disabled (oldest history, if any),
    // so a disabled agent never takes a slot from a live one.
    const rank = (server: Types.ServerListItem, mem: number | undefined) =>
      mem !== undefined
        ? mem
        : server.info.state === Types.ServerState.NotOk
          ? -1
          : server.info.state === Types.ServerState.Ok
            ? -2
            : -3;
    const label = (name: string, server: Types.ServerListItem) =>
      offline(server) ? `${name} (unreachable)` : name;
    const chartHosts = [...hosts].sort(
      (a, b) => Number(offline(a.primary)) - Number(offline(b.primary)),
    );
    const chartAgents = hosts.flatMap((host) =>
      host.servers.map((server) => ({ server, host })),
    );
    if (view === "hosts") {
      return {
        lines: [] as UsageLine[],
        hosts: chartHosts.slice(0, CANDIDATES),
        total: chartHosts.length,
        noun: "hosts",
        where: "",
      };
    }
    let lines: UsageLine[];
    let noun: string;
    if (view === "agents") {
      noun = "agents";
      lines = [...chartAgents]
        .sort(
          (a, b) =>
            rank(b.server, b.server.info.usage?.mem_bytes) -
            rank(a.server, a.server.info.usage?.mem_bytes),
        )
        .map(({ server, host }) => ({
          name: label(server.name, server),
          server: server.id,
          host,
        }));
    } else {
      noun = "stacks";
      const agents = new Map(chartAgents.map((a) => [a.server.id, a]));
      const agentHost = new Map(chartAgents.map((a) => [a.server.id, a.host]));
      lines = stacks
        .filter((stack) => agentHost.has(stack.info.server_id))
        .sort(
          (a, b) =>
            rank(
              agents.get(b.info.server_id)!.server,
              b.info.usage?.mem_bytes,
            ) -
            rank(agents.get(a.info.server_id)!.server, a.info.usage?.mem_bytes),
        )
        .map((stack) => ({
          name: label(stack.name, agents.get(stack.info.server_id)!.server),
          server: stack.info.server_id,
          host: agentHost.get(stack.info.server_id)!,
          usage: (record: Types.SystemStatsRecord) =>
            record.stacks?.find((s) => s.id === stack.id),
        }));
    }
    const where =
      hosts.length === 1
        ? `on ${hosts[0].name}`
        : `across ${hosts.length} hosts`;
    return {
      lines: lines.slice(0, CANDIDATES),
      hosts: [] as ServerHost[],
      total: lines.length,
      noun,
      where,
    };
  }, [view, hosts, stacks]);

  // A candidate with nothing stored in the window (a long disabled
  // agent, say) must not take a slot from one with history, so history
  // is fetched for up to CANDIDATES before choosing MAX_SERIES of them.
  const candidateIds = [
    ...plan.lines.map((line) => line.server),
    ...plan.hosts.map((host) => host.primary.id),
  ];
  const candidateQueries = useQueries({
    queries: [...new Set(candidateIds)].map((id) =>
      historicalStatsQuery(id, granularity),
    ),
  });
  const history = new Map<string, Types.SystemStatsRecord[]>();
  [...new Set(candidateIds)].forEach((id, i) =>
    history.set(id, candidateQueries[i].data?.stats ?? []),
  );
  const hasReading = (line: UsageLine) =>
    (history.get(line.server) ?? []).some((record) => {
      const usage = line.usage ? line.usage(record) : record.containers;
      return usage?.cpu_cores !== undefined || usage?.mem_bytes !== undefined;
    });
  const chartedLines = plan.lines.filter(hasReading).slice(0, MAX_SERIES);
  const chartedHostsOnly = plan.hosts
    .filter((host) => (history.get(host.primary.id) ?? []).length > 0)
    .slice(0, MAX_SERIES);
  // Load and disk stay host wide: one line for each host charted.
  const chartedHosts =
    view === "hosts"
      ? chartedHostsOnly
      : [...new Set(chartedLines.map((line) => line.host))];
  // The host lines of usage modes may need agents not fetched yet.
  const hostQueries = useQueries({
    queries: chartedHosts
      .map((host) => host.primary.id)
      .filter((id) => !history.has(id))
      .map((id) => historicalStatsQuery(id, granularity)),
  });
  chartedHosts
    .map((host) => host.primary.id)
    .filter((id) => !history.has(id))
    .forEach((id, i) => history.set(id, hostQueries[i]?.data?.stats ?? []));
  const isPending = [...candidateQueries, ...hostQueries].some(
    (query) => query.isPending,
  );
  const charted = view === "hosts" ? chartedHosts.length : chartedLines.length;
  const note =
    plan.total === 0
      ? `No ${plan.noun} ${plan.where}.`
      : plan.total > charted && !isPending
        ? view === "hosts"
          ? `Charting ${charted} of ${plan.total} hosts with history. Filter to specific Servers to compare the others.`
          : `Charting the top ${charted} of ${plan.total} ${plan.noun} ${plan.where} with history, by memory.`
        : undefined;

  const hostSeries: ChartSeries[] = chartedHosts.map((host) => ({
    name: host.name,
    color: hostColors.get(host.name) ?? SERIES_COLORS[0],
    stats: history.get(host.primary.id) ?? [],
  }));
  // Colors by rank, so the charted lines never share one.
  const usageSeries: ChartSeries[] = chartedLines.map((line, i) => ({
    name: line.name,
    color: SERIES_COLORS[i % SERIES_COLORS.length],
    stats: history.get(line.server) ?? [],
    usage: line.usage,
  }));

  return (
    <Section
      withBorder
      title="Historical"
      icon={<ChartLine size="1.3rem" />}
      titleRight={
        <Group ml={{ sm: "xl" }}>
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
          <SegmentedControl
            value={view}
            onChange={(value) => setView(value as HistoricalView)}
            data={HISTORICAL_VIEWS}
          />
        </Group>
      }
    >
      <Stack>
        {note && (
          <Text c="dimmed" size="sm">
            {note}
          </Text>
        )}
        <SimpleGrid cols={{ base: 1, xl: 2 }} spacing="xl">
          {view === "hosts" ? (
            HOST_CHARTS.map(([kind, icon]) => (
              <SeriesChart
                key={kind}
                kind={kind}
                icon={icon}
                series={hostSeries}
                isPending={isPending}
              />
            ))
          ) : (
            <>
              <SeriesChart
                kind="Cpu Cores"
                icon={<ICONS.Cpu size="1.1rem" />}
                note="cores"
                series={usageSeries}
                isPending={isPending}
              />
              <SeriesChart
                kind="Memory Bytes"
                icon={<ICONS.Memory size="1.1rem" />}
                note="working set"
                series={usageSeries}
                isPending={isPending}
              />
              <SeriesChart
                kind="Load Average"
                icon={<ICONS.LoadAvg size="1.1rem" />}
                note="host"
                series={hostSeries}
                isPending={isPending}
              />
              <SeriesChart
                kind="Disk"
                icon={<ICONS.Disk size="1.1rem" />}
                note="host"
                series={hostSeries}
                isPending={isPending}
              />
            </>
          )}
        </SimpleGrid>
      </Stack>
    </Section>
  );
}

/** One agent's or one stack's line on the usage charts. */
interface UsageLine {
  name: string;
  /** The agent whose history holds the line's readings. */
  server: string;
  host: ServerHost;
  usage?: ChartSeries["usage"];
}
