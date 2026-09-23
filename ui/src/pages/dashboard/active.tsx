import {
  actionStateIntention,
  procedureStateIntention,
  repoStateIntention,
  stackStateIntention,
} from "@/lib/color";
import { useRead } from "@/lib/hooks";
import { UsableResource } from "@/resources";
import ResourceLink from "@/resources/link";
import { DataTable, hexColorByIntention, SortableHeader } from "mogh_ui";
import { Section } from "mogh_ui";
import { StatusBadge } from "mogh_ui";
import { Types } from "komodo_client";
import { Circle } from "lucide-react";
import { ReactNode } from "react";

export default function DashboardActiveResources() {
  const stacks =
    useRead(
      "ListStacks",
      {
        query: {
          specific: {
            states: [
              Types.StackState.Deploying,
              Types.StackState.Restarting,
              Types.StackState.Removing,
            ],
          },
        },
        limit: 0,
      },
      { refetchInterval: 10_000 },
    ).data ?? [];
  const repos =
    useRead(
      "ListRepos",
      {
        query: {
          specific: {
            states: [Types.RepoState.Cloning, Types.RepoState.Pulling],
          },
        },
        limit: 0,
      },
      { refetchInterval: 10_000 },
    ).data ?? [];
  const procedures =
    useRead(
      "ListProcedures",
      {
        query: {
          specific: { states: [Types.ProcedureState.Running] },
        },
        limit: 0,
      },
      { refetchInterval: 10_000 },
    ).data ?? [];
  const actions =
    useRead(
      "ListActions",
      {
        query: {
          specific: { states: [Types.ActionState.Running] },
        },
        limit: 0,
      },
      { refetchInterval: 10_000 },
    ).data ?? [];

  const resources: {
    type: UsableResource;
    id: string;
    state: ReactNode;
  }[] = [
    ...stacks.map((stack) => ({
      type: "Stack" as UsableResource,
      id: stack.id,
      state: (
        <StatusBadge
          text={stack.info.state}
          intent={stackStateIntention(stack.info.state)}
        />
      ),
    })),
    ...repos.map((repo) => ({
      type: "Repo" as UsableResource,
      id: repo.id,
      state: (
        <StatusBadge
          text={repo.info.state}
          intent={repoStateIntention(repo.info.state)}
        />
      ),
    })),
    ...procedures.map((procedure) => ({
      type: "Procedure" as UsableResource,
      id: procedure.id,
      state: (
        <StatusBadge
          text={procedure.info.state}
          intent={procedureStateIntention(procedure.info.state)}
        />
      ),
    })),
    ...actions.map((action) => ({
      type: "Action" as UsableResource,
      id: action.id,
      state: (
        <StatusBadge
          text={action.info.state}
          intent={actionStateIntention(action.info.state)}
        />
      ),
    })),
  ];

  if (resources.length === 0) return null;

  return (
    <Section
      title="Active"
      mb="xl"
      icon={
        <Circle
          size="1rem"
          stroke={hexColorByIntention("Good")}
          fill={hexColorByIntention("Good")}
        />
      }
    >
      <DataTable
        tableKey="active-resources"
        data={resources}
        columns={[
          {
            accessorKey: "name",
            header: ({ column }) => (
              <SortableHeader column={column} title="Name" />
            ),
            cell: ({ row }) => (
              <ResourceLink type={row.original.type} id={row.original.id} />
            ),
          },
          {
            accessorKey: "type",
            header: ({ column }) => (
              <SortableHeader column={column} title="Resource" />
            ),
          },
          {
            header: "State",
            cell: ({ row }) => row.original.state,
          },
        ]}
      />
    </Section>
  );
}
