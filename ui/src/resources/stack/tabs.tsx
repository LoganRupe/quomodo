import { useLocalStorage } from "@mantine/hooks";
import { useStack } from ".";
import { usePermissions } from "@/lib/hooks";
import { Types } from "komodo_client";
import { useMemo } from "react";
import { MobileFriendlyTabsSelector, TabNoContent } from "mogh_ui";
import { Tabs } from "@mantine/core";
import { ICONS } from "@/lib/icons";
import { stackStateIntention } from "@/lib/color";
import StackConfig from "./config";
import StackInfo from "./info";
import StackServices from "./services";
import StackLog from "./log";

type StackTabsView = "Config" | "Info" | "Services" | "Log";

export default function StackTabs({ id }: { id: string }) {
  const [_view, setView] = useLocalStorage<StackTabsView>({
    key: `stack-${id}-tab-v2`,
    defaultValue: "Config",
  });
  const info = useStack(id)?.info;
  const { specificLogs } = usePermissions({
    type: "Stack",
    id,
  });

  const state = info?.state;
  const hideInfo = !info?.repo && !info?.linked_repo;
  const hideLogs =
    state === undefined ||
    state === Types.StackState.Unknown ||
    state === Types.StackState.Down ||
    !specificLogs;

  const view =
    (_view === "Info" && hideInfo) || (_view === "Log" && hideLogs)
      ? "Config"
      : _view;

  const tabs = useMemo<TabNoContent[]>(
    () => [
      {
        value: "Config",
        icon: ICONS.Settings,
      },
      {
        value: "Info",
        hidden: hideInfo,
        icon: ICONS.Search,
      },
      {
        value: "Services",
        icon: ICONS.Service,
      },
      {
        value: "Log",
        disabled: hideLogs,
        icon: ICONS.Log,
      },
    ],
    [hideInfo, hideLogs],
  );

  const Selector = (
    <MobileFriendlyTabsSelector
      tabs={tabs}
      value={view}
      onValueChange={setView as any}
    />
  );

  let View = Selector;
  switch (view) {
    case "Config":
      View = <StackConfig id={id} titleOther={Selector} />;
      break;
    case "Info":
      View = <StackInfo id={id} titleOther={Selector} />;
      break;
    case "Services":
      View = <StackServices id={id} titleOther={Selector} />;
      break;
    case "Log":
      View = <StackLog id={id} titleOther={Selector} />;
      break;
  }

  return (
    <Tabs color={stackStateIntention(state)} value={view}>
      {View}
    </Tabs>
  );
}
