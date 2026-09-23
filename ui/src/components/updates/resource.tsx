import { getUpdateQuery } from "@/lib/utils";
import { Types } from "komodo_client";
import { useMemo } from "react";
import UpdatesSection from "./section";

export default function ResourceUpdates({ type, id }: Types.ResourceTarget) {
  const query = useMemo(() => getUpdateQuery({ type, id }), [type, id]);

  return (
    <UpdatesSection query={query} link={`/updates?type=${type}&id=${id}`} />
  );
}
