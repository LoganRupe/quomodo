import { Fragment, ReactNode } from "react";
import { useStack } from ".";
import { useDashboardPreferences, useRead } from "@/lib/hooks";
import { Types } from "komodo_client";
import { Section } from "mogh_ui";
import { DataTable, SortableHeader } from "mogh_ui";
import StackServiceLink, {
  stackServiceStateIntention,
} from "@/components/stack-service-link";
import { StatusBadge } from "mogh_ui";
import DockerResourceLink from "@/components/docker/link";
import { Group, Text } from "@mantine/core";
import ContainerPorts from "@/components/docker/container-ports";
import { UsageCell, UsageKind, usageSortValue } from "@/components/usage";

/** Units that run containers, so they can have usage. */
const USAGE_KINDS = new Set<Types.QuadletKind>([
  Types.QuadletKind.Container,
  Types.QuadletKind.Pod,
  Types.QuadletKind.Kube,
]);

/** States where a unit's containers may be running. A stopped unit
 * has no usage to show, rather than a missing reading. */
const RUNNING_SERVICE_STATES = new Set<Types.StackServiceState>([
  Types.StackServiceState.Running,
  Types.StackServiceState.Unhealthy,
  Types.StackServiceState.Restarting,
  Types.StackServiceState.Paused,
]);

function ServiceUsageCell({
  service,
  kind,
}: {
  service: Types.StackService;
  kind: UsageKind;
}) {
  if (
    !service.usage &&
    !(
      USAGE_KINDS.has(service.unit.kind) &&
      RUNNING_SERVICE_STATES.has(service.state)
    )
  ) {
    return null;
  }
  return (
    <UsageCell
      usage={service.usage}
      kind={kind}
      showLimit={kind === "mem"}
      // A pod's row is its members summed.
      textProps={
        service.unit.kind === Types.QuadletKind.Pod ? { fw: 700 } : undefined
      }
    />
  );
}

export default function StackServices({
  id,
  titleOther,
}: {
  id: string;
  titleOther?: ReactNode;
}) {
  const info = useStack(id)?.info;
  const services =
    useRead("ListStackServices", { stack: id }, { refetchInterval: 10_000 })
      .data ?? [];

  return (
    <Section titleOther={titleOther} mb="md">
      {info?.server_id ? (
        <StackServicesTable
          stackId={id}
          serverId={info.server_id}
          services={services}
        />
      ) : (
        <></>
      )}
    </Section>
  );
}

function StackServicesTable({
  stackId,
  serverId,
  services,
}: {
  stackId: string;
  serverId: string;
  services: Types.ListStackServicesResponse;
}) {
  const showStats = !!useDashboardPreferences().preferences.showStackPageStats;
  return (
    <DataTable
      tableKey="StackServices"
      data={services}
      columns={[
        {
          accessorKey: "service",
          header: ({ column }) => (
            <SortableHeader column={column} title="Unit" />
          ),
          cell: ({ row }) => (
            <StackServiceLink id={stackId} service={row.original.service} />
          ),
        },
        {
          accessorKey: "unit.kind",
          header: ({ column }) => (
            <SortableHeader column={column} title="Kind" />
          ),
        },
        {
          accessorKey: "state",
          header: ({ column }) => (
            <SortableHeader column={column} title="State" />
          ),
          cell: ({ row }) => (
            <StatusBadge
              text={row.original.state}
              intent={stackServiceStateIntention(row.original.state)}
            />
          ),
        },
        showStats && {
          id: "Cpu",
          accessorFn: (row) => usageSortValue(row.usage, "cpu"),
          header: ({ column }) => (
            <SortableHeader column={column} title="CPU" sortDescFirst />
          ),
          cell: ({ row }) => (
            <ServiceUsageCell service={row.original} kind="cpu" />
          ),
        },
        showStats && {
          id: "Memory",
          accessorFn: (row) => usageSortValue(row.usage, "mem"),
          header: ({ column }) => (
            <SortableHeader column={column} title="Memory" sortDescFirst />
          ),
          cell: ({ row }) => (
            <ServiceUsageCell service={row.original} kind="mem" />
          ),
        },
        {
          accessorKey: "container.image",
          header: ({ column }) => (
            <SortableHeader column={column} title="Image" />
          ),
          cell: ({ row }) =>
            row.original.container ? (
              <DockerResourceLink
                type="Image"
                serverId={serverId}
                name={row.original.container.image}
                id={row.original.container.image_id}
              />
            ) : (
              row.original.unit.image && (
                <Text c="dimmed">{row.original.unit.image}</Text>
              )
            ),
        },
        {
          accessorKey: "unit.pod",
          header: ({ column }) => (
            <SortableHeader column={column} title="Pod" />
          ),
        },
        {
          accessorKey: "container.networks.0",
          header: ({ column }) => (
            <SortableHeader column={column} title="Networks" />
          ),
          cell: ({ row }) =>
            (row.original.container?.networks?.length ?? 0) > 0 && (
              <Group>
                {row.original.container?.networks?.map((network, i) => (
                  <Fragment key={network}>
                    <DockerResourceLink
                      type="Network"
                      serverId={serverId}
                      name={network}
                    />
                    {i !== row.original.container!.networks!.length - 1 && (
                      <div className="text-muted-foreground">|</div>
                    )}
                  </Fragment>
                ))}
              </Group>
            ),
        },
        {
          accessorKey: "container.ports.0",
          header: ({ column }) => (
            <SortableHeader column={column} title="Ports" />
          ),
          cell: ({ row }) => (
            <ContainerPorts
              ports={row.original.container?.ports ?? []}
              serverId={serverId}
            />
          ),
        },
      ]}
    />
  );
}
