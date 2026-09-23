import { useRead } from "@/lib/hooks";
import { ICONS } from "@/lib/icons";
import { Group, Text } from "@mantine/core";
import { Types } from "komodo_client";
import { ColorIntention, hexColorByIntention } from "mogh_ui";
import { Link } from "react-router-dom";

export function stackServiceStateIntention(
  state?: Types.StackServiceState,
): ColorIntention {
  switch (state) {
    case undefined:
      return "None";
    case Types.StackServiceState.Healthy:
    case Types.StackServiceState.Running:
      return "Good";
    case Types.StackServiceState.Paused:
    case Types.StackServiceState.Created:
      return "Warning";
    case Types.StackServiceState.Down:
      return "Neutral";
    case Types.StackServiceState.Unknown:
      return "Unknown";
    default:
      return "Critical";
  }
}

export interface StackServiceLinkProps {
  id: string;
  service: string;
}

export default function StackServiceLink({
  id,
  service: _service,
}: StackServiceLinkProps) {
  const services = useRead(
    "ListStackServices",
    { stack: id },
    { refetchInterval: 10_000 },
  ).data;
  const service = services?.find((s) => s.service === _service);
  const color = hexColorByIntention(stackServiceStateIntention(service?.state));
  return (
    <Group
      renderRoot={(props) => (
        <Link to={`/stacks/${id}/service/${_service}`} {...props} />
      )}
      onClick={(e) => e.stopPropagation()}
      wrap="nowrap"
      gap="xs"
    >
      <ICONS.Service size="1rem" color={color} />
      <Text className="hover-underline" style={{ textWrap: "nowrap" }}>
        {_service}
      </Text>
    </Group>
  );
}
