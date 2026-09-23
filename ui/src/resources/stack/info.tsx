import { ReactNode, useState } from "react";
import { useFullStack, useStack } from ".";
import { Types } from "komodo_client";
import { Section } from "mogh_ui";
import { Code, Group, Stack, Text } from "@mantine/core";
import { updateLogToHtml } from "@/lib/utils";
import { CopyButton } from "mogh_ui";
import { ShowHideButton } from "mogh_ui";
import { languageFromPath, MonacoEditor } from "mogh_ui";

export default function StackInfo({
  id,
  titleOther,
}: {
  id: string;
  titleOther: ReactNode;
}) {
  const [show, setShow] = useState<Record<string, boolean | undefined>>({});

  const notDown = useStack(id)?.info.state !== Types.StackState.Down;
  const stack = useFullStack(id);

  const latestContents = stack?.info?.remote_contents;
  const latestErrors = stack?.info?.remote_errors;
  const deployedContents = stack?.info?.deployed_contents;

  // Contents will be default hidden if there is more than 2 file to show
  const defaultShowContents = !latestContents || latestContents.length < 3;

  const toggleShow = (key: string) => () =>
    setShow((show) => ({
      ...show,
      [key]: !(show[key] ?? defaultShowContents),
    }));

  return (
    <Section titleOther={titleOther}>
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

      {/* Latest contents */}
      {latestContents &&
        latestContents.map((content) => (
          <FileContentsCard
            key={content.path}
            file={content}
            show={show[`latest-${content.path}`] ?? defaultShowContents}
            toggleShow={toggleShow(`latest-${content.path}`)}
          />
        ))}

      {/* Deployed contents */}
      {notDown && deployedContents && deployedContents.length > 0 && (
        <Stack className="bordered-light" bdrs="md" p="xl">
          <Stack ff="monospace" gap="0">
            <Text fz="md">Deployed contents:</Text>
            <Text fz="sm" c="dimmed">
              The unit files as they were when the Stack was last deployed.
            </Text>
          </Stack>
          {deployedContents.map((content) => (
            <FileContentsCard
              key={content.path}
              file={content}
              show={show[`deployed-${content.path}`] ?? false}
              toggleShow={toggleShow(`deployed-${content.path}`)}
            />
          ))}
        </Stack>
      )}
    </Section>
  );
}

function FileContentsCard({
  file,
  show,
  toggleShow,
}: {
  file: Types.FileContents;
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
