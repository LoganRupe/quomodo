import { BoxProps } from "@mantine/core";
import { Types } from "komodo_client";
import { useDashboardPreferences, useServerView } from "@/lib/hooks";
import StandardServerTable from "./standard";
import StatsServerTable from "./stats";
import HostServerTable from "./hosts";
import HostServerCards from "./cards";

export default function ServerTable({
  resources,
  onServerSort,
  ...boxProps
}: {
  resources: Types.ServerListItem[];
  /** When provided, sorting is handled server side,
   * and sort updates are passed to this callback. */
  onServerSort?: (sort: { sort_by?: string; sort_desc?: boolean }) => void;
} & BoxProps) {
  const { preferences } = useDashboardPreferences();
  const [view] = useServerView();
  // The host views group and sort on the client.
  if (view === "hosts") {
    return <HostServerTable resources={resources} {...boxProps} />;
  }
  if (view === "cards") {
    return <HostServerCards resources={resources} {...boxProps} />;
  }
  if (preferences.showServerStats) {
    return (
      <StatsServerTable
        resources={resources}
        onServerSort={onServerSort}
        {...boxProps}
      />
    );
  } else {
    return (
      <StandardServerTable
        resources={resources}
        onServerSort={onServerSort}
        {...boxProps}
      />
    );
  }
}
