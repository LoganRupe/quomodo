import { useListItem } from "@/lib/hooks";
import { ICONS } from "@/lib/icons";
import { Box, Group, HoverCard, Text } from "@mantine/core";
import { hexColorByIntention } from "mogh_ui";

/** Marks a Server whose agent runs as root. Shown only while the agent is
 * connected: a disconnected agent reports no user, root or not. */
export const ServerRootMarker = ({
  id,
  size = "1rem",
}: {
  id: string | undefined;
  size?: string | number;
}) => {
  const info = useListItem("Server", id)?.info;
  if (!info?.version || info.rootless_user) return null;
  const color = hexColorByIntention("Warning");
  return (
    <HoverCard position="bottom-start">
      <HoverCard.Target>
        <Box component="span" display="flex" style={{ flexShrink: 0 }}>
          <ICONS.Root size={size} color={color} />
        </Box>
      </HoverCard.Target>
      <HoverCard.Dropdown maw={320}>
        <Group gap="xs" mb="0.25rem" wrap="nowrap">
          <ICONS.Root size="1rem" color={color} />
          <Text fw="bold">Root agent</Text>
        </Group>
        <Text size="sm">
          This agent runs as root and deploys rootful quadlets. Its units can
          publish host ports below 1024, which a rootless agent refuses.
        </Text>
      </HoverCard.Dropdown>
    </HoverCard>
  );
};
