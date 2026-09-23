import { stackStateIntention } from "@/lib/color";
import {
  useInvalidate,
  useListItem,
  usePermissions,
  useRead,
  useWrite,
} from "@/lib/hooks";
import { ICONS } from "@/lib/icons";
import { Types } from "komodo_client";
import { hexColorByIntention, StatusBadge } from "mogh_ui";
import { RequiredResourceComponents } from "@/resources";
import StackTable from "./table";
import StackTabs from "./tabs";
import {
  DeployStack,
  DestroyStack,
  PullStack,
  RestartStack,
  StartStopStack,
} from "./executions";
import { useServer } from "@/resources/server";
import {
  ActionIcon,
  Box,
  Button,
  Center,
  Group,
  HoverCard,
  List,
  Loader,
  Text,
} from "@mantine/core";
import { notifications } from "@mantine/notifications";
import ResourceLink from "@/resources/link";
import RepoLink from "@/components/repo-link";
import HashCompare from "@/components/hash-compare";
import ResourceHeader from "../header";
import BatchExecutions from "@/resources/batch-executions";
import NewResourceWithDeployTarget from "../new-with-deploy-target";

export function useStack(
  id: string | undefined,
  useName?: boolean,
  refetchInterval?: number | false,
) {
  return useListItem("Stack", id, useName, refetchInterval);
}

export function useFullStack(id: string) {
  return useRead("GetStack", { stack: id }, { refetchInterval: 30_000 }).data;
}

/** Where the Stack sources its unit files from: a linked Repo, or a configured git repo. */
export function StackSource({
  info,
}: {
  info: Types.StackListItemInfo | undefined;
}) {
  if (!info) {
    return (
      <Center>
        <Loader size="sm" />
      </Center>
    );
  } else if (info.linked_repo) {
    return <ResourceLink type="Repo" id={info.linked_repo} />;
  } else if (info.repo) {
    return <RepoLink repo={info.repo} link={info.repo_link} />;
  } else {
    return (
      <Group gap="xs" wrap="nowrap">
        <ICONS.Alert size="1rem" />
        <Text fz={{ base: "sm", lg: "md" }}>No source</Text>
      </Group>
    );
  }
}

export const StackComponents: RequiredResourceComponents<
  Types.StackConfig,
  Types.StackInfo,
  Types.StackListItemInfo,
  Types.StackQuerySpecifics
> = {
  useList: (query, limit, page) =>
    useRead("ListStacks", { query, limit, page }).data,
  useListItem: useStack,
  useFull: useFullStack,

  useResourceLinks: (stack) => stack?.config?.links,

  useDashboardSummaryData: () => {
    const summary = useRead(
      "GetStacksSummary",
      {},
      { refetchInterval: 10_000 },
    ).data;
    const all = [
      summary?.running ?? 0,
      summary?.stopped ?? 0,
      summary?.unhealthy ?? 0,
      summary?.unknown ?? 0,
    ];
    const [running, stopped, unhealthy, unknown] = all;
    return [
      all.every((item) => item === 0) && {
        title: "Down",
        intention: "Neutral",
        value: summary?.down ?? 0,
      },
      { intention: "Good", value: running, title: "Running" },
      {
        intention: "Warning",
        value: stopped,
        title: "Stopped",
      },
      {
        intention: "Critical",
        value: unhealthy,
        title: "Unhealthy",
      },
      {
        intention: "Unknown",
        value: unknown,
        title: "Unknown",
      },
    ];
  },

  Description: () => <>Deploy podman quadlet units from a git repo.</>,

  New: ({ serverId, repoId }) => (
    <NewResourceWithDeployTarget
      type="Stack"
      serverId={serverId}
      repoId={repoId}
    />
  ),

  BatchExecutions: () => (
    <BatchExecutions
      type="Stack"
      executions={[
        ["PullStack", ICONS.Pull],
        ["DeployStack", ICONS.Deploy],
        ["RestartStack", ICONS.Restart],
        ["StopStack", ICONS.Stop],
        ["DestroyStack", ICONS.Destroy],
      ]}
    />
  ),

  Table: StackTable,

  Icon: ({ id, size = "1rem", noColor }) => {
    const info = useStack(id)?.info;
    const color = noColor
      ? undefined
      : info && hexColorByIntention(stackStateIntention(info.state));
    return <ICONS.Stack size={size} color={color} />;
  },

  ResourcePageHeader: ({ id }) => {
    const stack = useStack(id);
    return (
      <ResourceHeader
        type="Stack"
        id={id}
        resource={stack}
        intent={stackStateIntention(stack?.info.state)}
        icon={ICONS.Stack}
        name={stack?.name}
        state={stack?.info.state}
        status={
          stack?.info.state === Types.StackState.Unhealthy
            ? stack?.info.status
            : undefined
        }
      />
    );
  },

  State: ({ id }) => {
    const info = useStack(id)?.info;
    return (
      <StatusBadge
        text={info?.state}
        intent={stackStateIntention(info?.state)}
      />
    );
  },
  Info: {
    DeployTarget: ({ id }) => {
      const info = useStack(id)?.info;
      const server = useServer(info?.server_id);
      return server?.id ? (
        <ResourceLink type="Server" id={server?.id} />
      ) : (
        <Group gap="xs">
          <ICONS.Server size="1rem" />
          <Text>Unknown</Text>
        </Group>
      );
    },
    Source: ({ id }) => {
      const info = useStack(id)?.info;
      return <StackSource info={info} />;
    },
    Units: ({ id }) => {
      const units = useStack(id)?.info.units ?? [];
      return (
        <HoverCard position="bottom-start">
          <HoverCard.Target>
            <Text>
              <b>{units.length}</b> Unit
              {units.length !== 1 ? "s" : ""}
            </Text>
          </HoverCard.Target>
          <HoverCard.Dropdown>
            <List>
              {units.map((unit) => (
                <List.Item key={unit.file}>
                  <Group gap="xs">
                    <Text>{unit.file}</Text>
                    {unit.image && (
                      <>
                        <Text c="dimmed">-</Text>
                        <Text c="dimmed">{unit.image}</Text>
                      </>
                    )}
                  </Group>
                </List.Item>
              ))}
            </List>
          </HoverCard.Dropdown>
        </HoverCard>
      );
    },
    Hash: ({ id }) => {
      const fullInfo = useFullStack(id)?.info;
      const info = useStack(id)?.info;
      const state = info?.state;
      const stackDown =
        state === undefined ||
        state === Types.StackState.Unknown ||
        state === Types.StackState.Down;
      if (stackDown || info?.missing_units || !info?.latest_hash || !fullInfo) {
        return null;
      }
      return (
        // The border is added to the box.
        <Box>
          <HashCompare
            lastHash={fullInfo?.deployed_hash}
            lastMessage={fullInfo?.deployed_message}
            lastLabel="deployed"
            latestHash={fullInfo?.latest_hash}
            latestMessage={fullInfo?.latest_message}
          />
        </Box>
      );
    },
    NoConfig: ({ id }) => {
      const config = useFullStack(id)?.config;
      if (!config || config?.linked_repo || config?.repo) {
        return null;
      }
      return (
        <Box>
          <HoverCard width={300} position="bottom-start">
            <HoverCard.Target>
              <Button variant="filled" color="red">
                Config Missing
              </Button>
            </HoverCard.Target>
            <HoverCard.Dropdown>
              <Text>
                No source configured for stack. Cannot get stack state. Attach
                a Repo, or configure a git repo containing your quadlet unit
                files.
              </Text>
            </HoverCard.Dropdown>
          </HoverCard>
        </Box>
      );
    },
    MissingUnits: ({ id }) => {
      const info = useStack(id)?.info;
      if (!info?.missing_units || info.state === Types.StackState.Unknown) {
        return null;
      }
      return (
        <Box>
          <HoverCard width={300} position="bottom-start">
            <HoverCard.Target>
              <Button variant="filled" color="red">
                Units Missing
              </Button>
            </HoverCard.Target>
            <HoverCard.Dropdown>
              <Text>
                No quadlet unit files were found in the Run Directory, or the
                directory could not be read. Check the repo, branch and Run
                Directory, then refresh.
              </Text>
            </HoverCard.Dropdown>
          </HoverCard>
        </Box>
      );
    },
    RemoteErrors: ({ id }) => {
      const info = useFullStack(id)?.info;
      const errors = info?.remote_errors;
      if (!info || !errors || errors.length === 0) {
        return null;
      }
      return (
        <Box>
          <HoverCard width={300} position="bottom-start">
            <HoverCard.Target>
              <Button variant="filled" color="red">
                Remote Error
              </Button>
            </HoverCard.Target>
            <HoverCard.Dropdown>
              <Text>
                There are errors reading the remote file contents. See{" "}
                <b>Info</b> tab for details.
              </Text>
            </HoverCard.Dropdown>
          </HoverCard>
        </Box>
      );
    },
    Refresh: ({ id }) => {
      const inv = useInvalidate();
      const info = useStack(id)?.info;
      const { canExecute } = usePermissions({ type: "Stack", id });
      const { mutate, isPending } = useWrite("RefreshStackCache", {
        onSuccess: () => {
          inv(["ListStacks"], ["GetStack", { stack: id }]);
          notifications.show({ message: "Refreshed source file contents" });
        },
      });

      if (!canExecute || (!info?.linked_repo && !info?.repo)) return null;

      return (
        <ActionIcon onClick={() => mutate({ stack: id })} loading={isPending}>
          <ICONS.Refresh size="1rem" />
        </ActionIcon>
      );
    },
  },

  Executions: {
    DeployStack,
    PullStack,
    RestartStack,
    StartStopStack,
    DestroyStack,
  },

  Config: StackTabs,

  Page: {},
};
