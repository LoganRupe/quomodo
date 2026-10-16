import { Badge } from "@mantine/core";
import { Types } from "komodo_client";
import { agentLabel } from "./hosts";

/** Which user an agent runs as: "root", or the rootless user. */
export default function AgentBadge({
  server,
  count,
}: {
  server: Types.ServerListItem;
  /** Shown after the label, eg the agent's Stack count. */
  count?: number;
}) {
  const label = agentLabel(server);
  return (
    <Badge
      variant={
        server.info.state === Types.ServerState.NotOk ? "filled" : "light"
      }
      color={
        server.info.state === Types.ServerState.NotOk
          ? "red"
          : server.info.rootless_user
            ? "gray"
            : "blue"
      }
      radius="sm"
      tt="none"
      ff="monospace"
      style={{ flexShrink: 0 }}
      title={server.name}
    >
      {count === undefined ? label : `${label} ${count}`}
    </Badge>
  );
}
