import { ReactNode, useState } from "react";
import { useFullStack } from ".";
import { Types } from "komodo_client";
import { Section } from "mogh_ui";
import { Code, Group, SegmentedControl, Stack, Text } from "@mantine/core";
import { useLocalStorage } from "@mantine/hooks";
import { updateLogToHtml } from "@/lib/utils";
import { CopyButton } from "mogh_ui";
import { ShowHideButton } from "mogh_ui";
import { languageFromPath, MonacoEditor } from "mogh_ui";
import {
  StackFileDiffCard,
  StackFileStatus,
  StackFileStatusBadge,
  stackFileDiffs,
} from "@/components/stack-file-diffs";

type StackInfoView = "Changes" | "Files" | "Deployed";

export default function StackInfo({
  id,
  titleOther,
}: {
  id: string;
  titleOther: ReactNode;
}) {
  const [show, setShow] = useState<Record<string, boolean | undefined>>({});
  // Shared by every Stack, so a user who prefers one view keeps it.
  const [savedView, setView] = useLocalStorage<StackInfoView | undefined>({
    key: "stack-info-view-v1",
    defaultValue: undefined,
  });

  const stack = useFullStack(id);

  const latestContents = stack?.info?.remote_contents;
  const latestErrors = stack?.info?.remote_errors;
  const deployedContents = stack?.info?.deployed_contents;

  const diffs = stackFileDiffs(stack?.info);
  const changed = diffs?.filter((d) => d.status !== "same") ?? [];
  const statusByPath = Object.fromEntries(
    diffs?.map((d) => [d.path, d.status]) ?? [],
  );

  // Without a deploy to compare against there are only the latest files.
  const view: StackInfoView = !diffs
    ? "Files"
    : (savedView ?? (changed.length ? "Changes" : "Files"));

  // Contents will be default hidden if there is more than 2 file to show
  const defaultShowContents = !latestContents || latestContents.length < 3;

  const toggleShow = (key: string, fallback: boolean) => () =>
    setShow((show) => ({
      ...show,
      [key]: !(show[key] ?? fallback),
    }));

  return (
    <Section
      titleOther={titleOther}
      actions={
        diffs && (
          <SegmentedControl
            value={view}
            onChange={(view) => setView(view as StackInfoView)}
            data={[
              {
                value: "Changes",
                label: changed.length
                  ? `Changes (${changed.length})`
                  : "Changes",
              },
              "Files",
              "Deployed",
            ]}
          />
        )
      }
    >
      {/* Errors */}
      {latestErrors &&
        latestErrors.length > 0 &&
        latestErrors.map((error) => (
          <Stack key={error.path} className="bordered-light" bdrs="md" p="xl">
            <Group ff="monospace">
              <Text c="dimmed">Path:</Text>
              {error.path}
            </Group>
            <Code
              component="pre"
              fz="sm"
              mah={500}
              dangerouslySetInnerHTML={{
                __html: updateLogToHtml(error.contents),
              }}
              style={{ overflowY: "auto" }}
            />
          </Stack>
        ))}

      {/* Latest vs deployed */}
      {view === "Changes" && (
        <>
          <Text c={changed.length ? "yellow" : "dimmed"} ff="monospace">
            {changed.length
              ? `${changed.length} of ${diffs?.length} files differ from the last deploy. Deploying applies them.`
              : "Every file matches the last deploy."}
          </Text>
          {changed.map((diff) => (
            <StackFileDiffCard
              key={diff.path}
              diff={diff}
              defaultShow={changed.length < 3}
            />
          ))}
        </>
      )}

      {/* Latest contents */}
      {view === "Files" &&
        latestContents &&
        latestContents.map((content) => (
          <FileContentsCard
            key={content.path}
            file={content}
            status={statusByPath[content.path]}
            show={show[`latest-${content.path}`] ?? defaultShowContents}
            toggleShow={toggleShow(
              `latest-${content.path}`,
              defaultShowContents,
            )}
          />
        ))}

      {/* Deployed contents */}
      {view === "Deployed" && (
        <>
          <Text c="dimmed" ff="monospace">
            The files as they were when the Stack was last deployed.
          </Text>
          {deployedContents?.map((content) => (
            <FileContentsCard
              key={content.path}
              file={content}
              show={show[`deployed-${content.path}`] ?? false}
              toggleShow={toggleShow(`deployed-${content.path}`, false)}
            />
          ))}
        </>
      )}
    </Section>
  );
}

function FileContentsCard({
  file,
  status,
  show,
  toggleShow,
}: {
  file: Types.FileContents;
  status?: StackFileStatus;
  show: boolean;
  toggleShow: () => void;
}) {
  return (
    <Stack className="bordered-light" bdrs="md" p="xl">
      <Group
        justify="space-between"
        style={{ cursor: "pointer" }}
        onClick={toggleShow}
        tabIndex={0}
        role="button"
        aria-pressed={show}
        onKeyDown={(e) => {
          if (
            (e.key === "Enter" || e.key === " ") &&
            e.target === e.currentTarget
          ) {
            if (e.key === " ") e.preventDefault();
            toggleShow();
          }
        }}
      >
        <Group ff="monospace">
          <Text c="dimmed">File:</Text>
          {file.path}
          <CopyButton label="file path" content={file.path} />
          {status && <StackFileStatusBadge status={status} />}
        </Group>
        <ShowHideButton show={show} setShow={toggleShow} />
      </Group>

      {show && (
        <MonacoEditor
          value={file.contents}
          language={languageFromPath(file.path)}
          filename={file.path}
          readOnly
        />
      )}
    </Stack>
  );
}
