import { useState } from "react";
import NewResource from "./new";
import { Types } from "komodo_client";
import ResourceSelector from "./selector";

/** Used by Stacks */
export default function NewResourceWithDeployTarget({
  type,
  serverId: _serverId,
  repoId,
}: {
  type: "Stack";
  serverId?: string;
  repoId?: string;
}) {
  const [serverId, setServerId] = useState("");
  return (
    <NewResource<Types.StackConfig>
      type={type}
      config={() => ({
        server_id: _serverId ?? serverId,
        linked_repo: repoId,
      })}
      extraInputs={
        !_serverId ? (
          <ResourceSelector
            type="Server"
            selected={serverId}
            onSelect={setServerId}
            targetProps={{ w: "100%", maw: "100%" }}
            width="target"
            position="bottom"
            clearable
          />
        ) : undefined
      }
      showTemplateSelector={!!_serverId || !serverId}
    />
  );
}
