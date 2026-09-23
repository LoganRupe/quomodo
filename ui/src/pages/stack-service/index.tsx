import { Center, Text } from "@mantine/core";
import { useParams } from "react-router-dom";
import {
  DeployStack,
  PullStack,
  RestartStack,
  StartStopStack,
} from "@/resources/stack/executions";
import { useStack } from "@/resources/stack";
import {
  useContainerPortsMap,
  usePermissions,
  useRead,
  useSetTitle,
} from "@/lib/hooks";
import { ICONS } from "@/lib/icons";
import ResourceLink from "@/resources/link";
import DockerResourceLink from "@/components/docker/link";
import { ContainerPort } from "@/components/docker/container-ports";
import { stackServiceStateIntention } from "@/components/stack-service-link";
import LogSection from "@/components/log-section";
import ResourceSubPage from "@/resources/sub-page";

type IdServiceComponent = React.FC<{ id: string; service?: string }>;

const Executions: { [action: string]: IdServiceComponent } = {
  DeployStack,
  PullStack,
  RestartStack,
  StartStopStack,
};

export default function StackService() {
  const {
    type,
    id: stackId,
    service: serviceName,
  } = useParams() as {
    type: string;
    id: string;
    service: string;
  };
  if (type !== "stacks") {
    return (
      <Center h="50vh">
        <Text>This resource type does not have any services.</Text>
      </Center>
    );
  }
  return <StackServiceInner stackId={stackId} serviceName={serviceName} />;
}

function StackServiceInner({
  stackId,
  serviceName,
}: {
  stackId: string;
  serviceName: string;
}) {
  const stack = useStack(stackId);
  useSetTitle(`${stack?.name} | ${serviceName}`);
  const { specificLogs } = usePermissions({ type: "Stack", id: stackId });
  const services = useRead(
    "ListStackServices",
    { stack: stackId },
    { refetchInterval: 10_000 },
  ).data;
  const service = services?.find((s) => s.service === serviceName);

  const container = service?.container;
  const unit = service?.unit;
  const serverId = stack?.info.server_id;

  const portsMap = useContainerPortsMap(container?.ports ?? []);

  const intention = stackServiceStateIntention(service?.state);

  return (
    <ResourceSubPage
      entityTypeName="Stack Unit"
      parentType="Stack"
      parentId={stackId}
      name={serviceName}
      icon={ICONS.Service}
      intent={intention}
      state={service?.state}
      status={container?.status}
      info={
        <>
          {unit && <Text c="dimmed">{unit.kind}</Text>}
          {unit?.service && <Text ff="monospace">{unit.service}</Text>}
          {serverId && (
            <>
              <ResourceLink type="Server" id={serverId} />
              {container?.name && (
                <DockerResourceLink
                  type="Container"
                  serverId={serverId}
                  name={container.name}
                />
              )}
              {(container?.image || unit?.image) && (
                <DockerResourceLink
                  type="Image"
                  serverId={serverId}
                  name={container?.image ?? unit?.image}
                  id={container?.image_id}
                />
              )}
              {(container?.pod || unit?.pod) && (
                <Text>Pod: {container?.pod ?? unit?.pod}</Text>
              )}
              {container?.networks?.map((network) => (
                <DockerResourceLink
                  key={network}
                  type="Network"
                  serverId={serverId}
                  name={network}
                />
              ))}
              {container?.volumes?.map((volume) => (
                <DockerResourceLink
                  key={volume}
                  type="Volume"
                  serverId={serverId}
                  name={volume}
                />
              ))}
              {Object.keys(portsMap).map((hostPort) => (
                <ContainerPort
                  key={hostPort}
                  hostPort={hostPort}
                  ports={portsMap[hostPort]}
                  serverId={serverId}
                />
              ))}
            </>
          )}
        </>
      }
      executions={
        <>
          {Object.entries(Executions).map(([key, Execution]) => (
            <Execution key={key} id={stackId} service={serviceName} />
          ))}
        </>
      }
    >
      <LogSection
        title="Log"
        icon={<ICONS.Log size="1.3rem" />}
        target={{ type: "Stack", stackId, services: [serviceName] }}
        disabled={!specificLogs || !container}
      />
    </ResourceSubPage>
  );
}
