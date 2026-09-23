import {
  useExecute,
  useInvalidate,
  usePermissions,
  useRead,
} from "@/lib/hooks";
import { useServer } from ".";
import { ConfirmButton } from "mogh_ui";
import { ICONS } from "@/lib/icons";
import ConfirmModalWithDisable from "@/components/confirm-modal-with-disable";
import { EXECUTION_ACTION_STATE_REQUERY_MS } from "@/lib/utils";

export const Prune = ({
  serverId,
  type,
}: {
  serverId: string;
  type: "Images" | "System";
}) => {
  const server = useServer(serverId);
  const invalidate = useInvalidate();
  const { mutateAsync: prune, isPending } = useExecute(`Prune${type}`, {
    onSuccess: () =>
      setTimeout(
        () => invalidate(["GetServerActionState"]),
        EXECUTION_ACTION_STATE_REQUERY_MS,
      ),
  });
  const action_state = useRead(
    "GetServerActionState",
    { server: serverId },
    { refetchInterval: 5_000 },
  ).data;
  const { canExecute } = usePermissions({ type: "Server", id: serverId });

  if (!server) return;

  const pruning =
    type === "Images"
      ? action_state?.pruning_images
      : action_state?.pruning_system;

  const pending = isPending || !!pruning;

  if (type === "Images") {
    return (
      <ConfirmButton
        icon={<ICONS.Prune size="1rem" />}
        onClick={() => prune({ server: serverId })}
        loading={pending}
        disabled={!canExecute || pending}
      >
        {`Prune ${type}`}
      </ConfirmButton>
    );
  } else {
    return (
      <ConfirmModalWithDisable
        confirmText={server?.name}
        icon={<ICONS.Prune size="1rem" />}
        onConfirm={() => prune({ server: serverId })}
        loading={pending}
        disabled={!canExecute || pending}
      >
        {`Prune ${type}`}
      </ConfirmModalWithDisable>
    );
  }
};
