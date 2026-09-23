import { Fragment, ReactNode } from "react";
import { useStack } from ".";
import { useRead } from "@/lib/hooks";
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
