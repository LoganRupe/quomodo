import { useRead } from "@/lib/hooks";
import { useLocalStorage } from "@mantine/hooks";
import { useServer } from ".";
import { useMemo } from "react";
import { MobileFriendlyTabsSelector, TabNoContent } from "mogh_ui";
import { Types } from "komodo_client";
import { Tabs } from "@mantine/core";
import { serverStateIntention } from "@/lib/color";
import ServerConfig from "./config";
import { ICONS } from "@/lib/icons";
import ServerContainers from "./docker";
import ServerStats from "./stats";
import ServerHostedResourcesSection from "./resources";

type ServerTabsView = "Config" | "Stats" | "Containers" | "Resources";

export default function ServerTabs({ id }: { id: string }) {
  const [view, setView] = useLocalStorage<ServerTabsView>({
    key: `server-${id}-tab-v2`,
    defaultValue: "Config",
  });

  const coreVersion = useRead("GetVersion", {}).data?.version;
  const serverInfo = useServer(id)?.info;
  const notReachable = serverInfo?.state !== Types.ServerState.Ok;

  const stacks =
    useRead("ListStacks", {
      query: { specific: { server_ids: [id] } },
      limit: 1,
    }).data ?? [];
  const noStacks = stacks.length === 0;
  const repos =
    useRead("ListRepos", {
      query: { specific: { server_ids: [id] } },
      limit: 1,
    }).data ?? [];
  const noRepos = repos.length === 0;

  const noResources = noRepos && noStacks;

  const tabs = useMemo<TabNoContent[]>(
    () => [
      {
        value: "Config",
        icon: ICONS.Settings,
      },
      {
        value: "Stats",
        icon: ICONS.Stats,
      },
      {
        value: "Containers",
        icon: ICONS.Container,
        disabled: notReachable,
      },
      {
        value: "Resources",
        icon: ICONS.Resources,
        disabled: noResources,
      },
    ],
    [noResources, notReachable],
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
      View = <ServerConfig id={id} titleOther={Selector} />;
      break;
    case "Stats":
      View = <ServerStats id={id} titleOther={Selector} />;
      break;
    case "Containers":
      View = <ServerContainers id={id} titleOther={Selector} />;
      break;
    case "Resources":
      View = (
        <ServerHostedResourcesSection serverId={id} titleOther={Selector} />
      );
      break;
  }

  return (
    <Tabs
      color={serverStateIntention(
        serverInfo?.state,
        !!coreVersion &&
          !!serverInfo?.version &&
          coreVersion !== serverInfo.version,
      )}
      value={view}
    >
      {View}
    </Tabs>
  );
}
