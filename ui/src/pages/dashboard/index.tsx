import { Page } from "mogh_ui";
import {
  useDashboardPreferences,
  useSetTitle,
  useTagsFilter,
} from "@/lib/hooks";
import { ICONS } from "@/lib/icons";
import { Group } from "@mantine/core";
import DashboardRecents from "./recents";
import ExportToml from "@/components/export-toml";
import ServerShowStats from "@/resources/server/show-stats";
import ShowStatsToggle from "@/components/show-stats-toggle";
import ShowTables from "./show-tables";
import DashboardTables from "./tables";
import DashboardActiveResources from "./active";

export default function Dashboard() {
  const { preferences } = useDashboardPreferences();
  const tags = useTagsFilter();
  useSetTitle(undefined);
  return (
    <Page
      aboveTitle={<DashboardActiveResources />}
      title="Dashboard"
      icon={ICONS.Dashboard}
      oppositeTitle={
        <Group w={{ base: "100%", xs: "fit-content" }}>
          <ShowTables />
          <ServerShowStats />
          <ShowStatsToggle preference="showStackStats" label="Stack Stats" />
          <ExportToml tags={preferences.showTables ? tags : undefined} />
        </Group>
      }
    >
      {preferences.showTables ? <DashboardTables /> : <DashboardRecents />}
    </Page>
  );
}
