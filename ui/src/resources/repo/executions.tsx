import { useExecute, useInvalidate, useRead } from "@/lib/hooks";
import { useRepo } from ".";
import { ConfirmButton } from "mogh_ui";
import { ICONS } from "@/lib/icons";
import { EXECUTION_ACTION_STATE_REQUERY_MS } from "@/lib/utils";

const useInvalidateActionState = () => {
  const invalidate = useInvalidate();
  return {
    onSuccess: () =>
      setTimeout(
        () => invalidate(["GetRepoActionState"]),
        EXECUTION_ACTION_STATE_REQUERY_MS,
      ),
  };
};

export function CloneRepo({ id }: { id: string }) {
  const { mutate, isPending } = useExecute(
    "CloneRepo",
    useInvalidateActionState(),
  );
  const cloning = useRead(
    "GetRepoActionState",
    { repo: id },
    { refetchInterval: 5_000 },
  ).data?.cloning;
  const info = useRepo(id)?.info;
  if (!info?.server_id) return null;
  const hash = info?.latest_hash;
  const isCloned = (hash?.length || 0) > 0;
  const pending = isPending || cloning;
  return (
    <ConfirmButton
      icon={<ICONS.CloneRepo size="1rem" />}
      onClick={() => mutate({ repo: id })}
      disabled={pending}
      loading={pending}
    >
      {isCloned ? "Reclone" : "Clone"}
    </ConfirmButton>
  );
}

export function PullRepo({ id }: { id: string }) {
  const { mutate, isPending } = useExecute(
    "PullRepo",
    useInvalidateActionState(),
  );
  const pulling = useRead(
    "GetRepoActionState",
    { repo: id },
    { refetchInterval: 5_000 },
  ).data?.pulling;
  const info = useRepo(id)?.info;
  if (!info?.server_id) return null;
  const hash = info?.latest_hash;
  const isCloned = (hash?.length || 0) > 0;
  if (!isCloned) return null;
  const pending = isPending || pulling;
  return (
    <ConfirmButton
      icon={<ICONS.PullRepo size="1rem" />}
      onClick={() => mutate({ repo: id })}
      disabled={pending}
      loading={pending}
    >
      Pull
    </ConfirmButton>
  );
}
