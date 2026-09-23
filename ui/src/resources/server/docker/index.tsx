import { atom, useAtom } from "jotai";
import { ReactNode } from "react";
import { useServer } from "..";
import { Types } from "komodo_client";
import { Section } from "mogh_ui";
import { Center, Stack, Text } from "@mantine/core";
import ContainersSection from "@/components/docker/containers-section";
import { useRead } from "@/lib/hooks";

const searchAtom = atom("");
export function useServerDockerSearch() {
  return useAtom(searchAtom);
}

export default function ServerContainers({
  id,
  titleOther,
}: {
  id: string;
  titleOther: ReactNode;
}) {
  const state = useServer(id)?.info.state ?? Types.ServerState.NotOk;
  const _search = useServerDockerSearch();
  const containers =
    useRead(
      "ListContainers",
      { server: id },
      {
        refetchInterval: 10_000,
        enabled: state === Types.ServerState.Ok,
      },
    ).data ?? [];

  if ([Types.ServerState.NotOk, Types.ServerState.Disabled].includes(state)) {
    return (
      <Section titleOther={titleOther}>
        <Center h="20vh">
          <Stack align="center" justify="center" gap="0">
            <Text fz="h2">Server unreachable</Text>
            <Text c="dimmed">Containers are not available</Text>
          </Stack>
        </Center>
      </Section>
    );
  }

  return (
    <ContainersSection
      serverId={id}
      containers={containers}
      titleOther={titleOther}
      _search={_search}
      forceTall
    />
  );
}
