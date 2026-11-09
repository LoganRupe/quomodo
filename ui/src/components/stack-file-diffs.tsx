import { useState } from "react";
import { Badge, Button, Group, Stack, Text } from "@mantine/core";
import { Types } from "komodo_client";
import {
  languageFromPath,
  MonacoDiffEditor,
  MonacoEditor,
  ShowHideButton,
} from "mogh_ui";
import { useRead } from "@/lib/hooks";

export type StackFileStatus = "changed" | "new" | "removed" | "same";

export interface StackFileDiff {
  path: string;
  status: StackFileStatus;
  /** The contents at the last deploy. Empty for a new file. */
  deployed: string;
  /** The contents at Core's last refresh. Empty for a removed file. */
  latest: string;
}

/**
 * Compares the files from Core's last refresh of a Stack with the ones
 * recorded at its last deploy, the same way a ResourceSync decides to
 * redeploy it (`bin/core/src/sync/deploy.rs`).
 * Undefined when there is no deploy or no refresh to compare.
 */
export function stackFileDiffs(
  info: Types.StackInfo | undefined,
): StackFileDiff[] | undefined {
  const deployed = info?.deployed_contents;
  const latest = info?.remote_contents;
  if (!deployed || !latest) return undefined;
  const diffs: StackFileDiff[] = latest.map((file) => {
    const match = deployed.find((d) => d.path === file.path);
    return {
      path: file.path,
      status: !match
        ? "new"
        : match.contents === file.contents
          ? "same"
          : "changed",
      deployed: match?.contents ?? "",
      latest: file.contents,
    };
  });
  for (const file of deployed) {
    if (!latest.some((l) => l.path === file.path)) {
      diffs.push({
        path: file.path,
        status: "removed",
        deployed: file.contents,
        latest: "",
      });
    }
  }
  return diffs;
}

const BADGES: Record<
  Exclude<StackFileStatus, "same">,
  { color: string; label: string }
> = {
  changed: { color: "yellow", label: "changed" },
  new: { color: "blue", label: "new" },
  removed: { color: "red", label: "removed" },
};

export function StackFileStatusBadge({ status }: { status: StackFileStatus }) {
  if (status === "same") return null;
  const { color, label } = BADGES[status];
  return (
    <Badge color={color} variant="light">
      {label}
    </Badge>
  );
}

/** One file as a diff of the deployed copy (left) against the latest (right). */
export function StackFileDiffCard({
  diff,
  defaultShow,
}: {
  diff: StackFileDiff;
  defaultShow: boolean;
}) {
  const [show, setShow] = useState(defaultShow);
  return (
    <Stack className="bordered-light" bdrs="md" p="xl">
      <Group justify="space-between">
        <Group ff="monospace" gap="xs">
          <Text c="dimmed">File:</Text>
          {diff.path}
          <StackFileStatusBadge status={diff.status} />
        </Group>
        <ShowHideButton show={show} setShow={() => setShow((s) => !s)} />
      </Group>
      {/* A new or removed file has only one side to show. */}
      {show && diff.status !== "changed" && (
        <MonacoEditor
          value={diff.status === "removed" ? diff.deployed : diff.latest}
          language={languageFromPath(diff.path)}
          filename={diff.path}
          readOnly
        />
      )}
      {/* The editor's height counts every line, so folding the unchanged
          ones would leave a gap. */}
      {show && diff.status === "changed" && (
        <MonacoDiffEditor
          original={diff.deployed}
          modified={diff.latest}
          language={languageFromPath(diff.path)}
          hideUnchangedRegions={false}
          readOnly
        />
      )}
    </Stack>
  );
}

/**
 * Under a Stack a ResourceSync will deploy: the files that differ from the
 * last deploy, collapsed. Renders nothing if the user can't read the Stack
 * or no file differs.
 */
export function StackPendingFileDiffs({ stack }: { stack: string }) {
  const [show, setShow] = useState(false);
  const info = useRead("GetStack", { stack }).data?.info;
  const changed = stackFileDiffs(info)?.filter((d) => d.status !== "same");
  if (!changed?.length) return null;
  return (
    <Stack w="100%" gap="xs">
      <Button
        variant="subtle"
        size="compact-sm"
        w="fit-content"
        onClick={() => setShow((s) => !s)}
      >
        {show ? "Hide" : "Show"} {changed.length} changed file
        {changed.length === 1 ? "" : "s"}
      </Button>
      {show &&
        changed.map((diff) => (
          <StackFileDiffCard key={diff.path} diff={diff} defaultShow />
        ))}
    </Stack>
  );
}
