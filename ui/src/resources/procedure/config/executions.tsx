import { useRead } from "@/lib/hooks";
import ResourceSelector from "@/resources/selector";
import { ICONS } from "@/lib/icons";
import { TextUpdateModal } from "mogh_ui";
import { Group, MultiSelect, Switch, TextInput } from "@mantine/core";
import { notifications } from "@mantine/notifications";
import { Types } from "komodo_client";
import { useEffect, useState } from "react";
import TagMultiSelector from "@/components/tags/multi-selector";

export type ExecutionType = Types.Execution["type"];

export type ProcedureExecutionComponent<
  T extends ExecutionType,
  P = Extract<Types.Execution, { type: T }>["params"],
> = React.FC<{
  params: P;
  setParams: React.Dispatch<React.SetStateAction<P>>;
  disabled: boolean;
}>;

export type ProcedureMinExecutionType = Exclude<
  ExecutionType,
  "CancelProcedure" | "CancelAction"
>;

export type ProcedureExecutionParams<T extends ProcedureMinExecutionType> =
  Extract<Types.Execution, { type: T }>["params"];

export type ProcedureExecutions = {
  [ExType in ProcedureMinExecutionType]: {
    Component: ProcedureExecutionComponent<ExType>;
    params: ProcedureExecutionParams<ExType>;
  };
};

export const PROCEDURE_EXECUTIONS: ProcedureExecutions = {
  None: {
    params: {},
    Component: () => <></>,
  },
  RunProcedure: {
    params: { procedure: "" },
    Component: ({ params, setParams, disabled }) => (
      <ResourceSelector
        type="Procedure"
        selected={params.procedure}
        onSelect={(procedure) => setParams({ procedure })}
        disabled={disabled}
      />
    ),
  },
  BatchRunProcedure: {
    params: { pattern: "", tags: [] },
    Component: ({ params, setParams, disabled }) => (
      <Group>
        <TextUpdateModal
          title="Match procedures"
          value={
            params.pattern ||
            "# Match procedures by name, id, wildcard, or \\regex\\.\n"
          }
          onUpdate={(pattern) =>
            setParams({ ...params, pattern })
          }
          disabled={disabled}
          useMonaco
          monacoLanguage="string_list"
        />
        <TagMultiSelector
          title="Match Tags"
          value={params.tags ?? []}
          onChange={(tags) => setParams({ ...params, tags })}
          disabled={disabled}
          useName
        />
      </Group>
    ),
  },
  RunAction: {
    params: { action: "", args: {} },
    Component: ({ params, setParams, disabled }) => (
      <Group>
        <ResourceSelector
          type="Action"
          selected={params.action}
          onSelect={(action) => setParams({ action, args: params.args })}
          disabled={disabled}
        />
        <TextUpdateModal
          title="Action Arguments (JSON)"
          value={JSON.stringify(params.args ?? {}, undefined, 2)}
          onUpdate={(args) =>
            setParams({ action: params.action, args: JSON.parse(args) })
          }
          disabled={disabled}
          useMonaco
          monacoLanguage="json"
        />
      </Group>
    ),
  },
  BatchRunAction: {
    params: { pattern: "", tags: [] },
    Component: ({ params, setParams, disabled }) => (
      <Group>
        <TextUpdateModal
          title="Match actions"
          value={
            params.pattern ||
            "# Match actions by name, id, wildcard, or \\regex\\.\n"
          }
          onUpdate={(pattern) =>
            setParams({ ...params, pattern })
          }
          disabled={disabled}
          useMonaco
          monacoLanguage="string_list"
        />
        <TagMultiSelector
          title="Match Tags"
          value={params.tags ?? []}
          onChange={(tags) => setParams({ ...params, tags })}
          disabled={disabled}
          useName
        />
      </Group>
    ),
  },
  DeployStack: {
    params: { stack: "", services: [] },
    Component: ({ params, setParams, disabled }) => {
      const allServices = useRead("ListStackServices", {
        stack: params.stack,
      }).data?.map((s) => s.service);
      return (
        <Group>
          <ResourceSelector
            type="Stack"
            selected={params.stack}
            onSelect={(id) =>
              setParams(
                id ? { ...params, stack: id } : { stack: id, services: [] },
              )
            }
            disabled={disabled}
          />
          <MultiSelect
            leftSection={<ICONS.Service size="1rem" />}
            placeholder={params.services?.length ? undefined : "All services"}
            value={params.services}
            data={allServices}
            onChange={(services) => setParams({ ...params, services })}
            styles={{ inputField: { width: 130 } }}
            searchable
            clearable
          />
        </Group>
      );
    },
  },
  BatchDeployStack: {
    params: { pattern: "", tags: [] },
    Component: ({ params, setParams, disabled }) => (
      <Group>
        <TextUpdateModal
          title="Match stacks"
          value={
            params.pattern ||
            "# Match stacks by name, id, wildcard, or \\regex\\.\n"
          }
          onUpdate={(pattern) =>
            setParams({ ...params, pattern })
          }
          disabled={disabled}
          useMonaco
          monacoLanguage="string_list"
        />
        <TagMultiSelector
          title="Match Tags"
          value={params.tags ?? []}
          onChange={(tags) => setParams({ ...params, tags })}
          disabled={disabled}
          useName
        />
      </Group>
    ),
  },
  DeployStackIfChanged: {
    params: { stack: "" },
    Component: ({ params, setParams, disabled }) => (
      <ResourceSelector
        type="Stack"
        selected={params.stack}
        onSelect={(id) => setParams({ stack: id })}
        disabled={disabled}
      />
    ),
  },
  BatchDeployStackIfChanged: {
    params: { pattern: "", tags: [] },
    Component: ({ params, setParams, disabled }) => (
      <Group>
        <TextUpdateModal
          title="Match stacks"
          value={
            params.pattern ||
            "# Match stacks by name, id, wildcard, or \\regex\\.\n"
          }
          onUpdate={(pattern) =>
            setParams({ ...params, pattern })
          }
          disabled={disabled}
          useMonaco
          monacoLanguage="string_list"
        />
        <TagMultiSelector
          title="Match Tags"
          value={params.tags ?? []}
          onChange={(tags) => setParams({ ...params, tags })}
          disabled={disabled}
          useName
        />
      </Group>
    ),
  },
  PullStack: {
    params: { stack: "" },
    Component: ({ params, setParams, disabled }) => {
      const allServices = useRead("ListStackServices", {
        stack: params.stack,
      }).data?.map((s) => s.service);
      return (
        <Group>
          <ResourceSelector
            type="Stack"
            selected={params.stack}
            onSelect={(id) =>
              setParams(
                id ? { ...params, stack: id } : { stack: id, services: [] },
              )
            }
            disabled={disabled}
          />
          <MultiSelect
            leftSection={<ICONS.Service size="1rem" />}
            placeholder={params.services?.length ? undefined : "All services"}
            value={params.services}
            data={allServices}
            onChange={(services) => setParams({ ...params, services })}
            styles={{ inputField: { width: 130 } }}
            searchable
            clearable
          />
        </Group>
      );
    },
  },
  BatchPullStack: {
    params: { pattern: "", tags: [] },
    Component: ({ params, setParams, disabled }) => (
      <Group>
        <TextUpdateModal
          title="Match stacks"
          value={
            params.pattern ||
            "# Match stacks by name, id, wildcard, or \\regex\\.\n"
          }
          onUpdate={(pattern) =>
            setParams({ ...params, pattern })
          }
          disabled={disabled}
          useMonaco
          monacoLanguage="string_list"
        />
        <TagMultiSelector
          title="Match Tags"
          value={params.tags ?? []}
          onChange={(tags) => setParams({ ...params, tags })}
          disabled={disabled}
          useName
        />
      </Group>
    ),
  },
  StartStack: {
    params: { stack: "" },
    Component: ({ params, setParams, disabled }) => {
      const allServices = useRead("ListStackServices", {
        stack: params.stack,
      }).data?.map((s) => s.service);
      return (
        <Group>
          <ResourceSelector
            type="Stack"
            selected={params.stack}
            onSelect={(id) =>
              setParams(
                id ? { ...params, stack: id } : { stack: id, services: [] },
              )
            }
            disabled={disabled}
          />
          <MultiSelect
            leftSection={<ICONS.Service size="1rem" />}
            placeholder={params.services?.length ? undefined : "All services"}
            value={params.services}
            data={allServices}
            onChange={(services) => setParams({ ...params, services })}
            styles={{ inputField: { width: 130 } }}
            searchable
            clearable
          />
        </Group>
      );
    },
  },
  RestartStack: {
    params: { stack: "" },
    Component: ({ params, setParams, disabled }) => {
      const allServices = useRead("ListStackServices", {
        stack: params.stack,
      }).data?.map((s) => s.service);
      return (
        <Group>
          <ResourceSelector
            type="Stack"
            selected={params.stack}
            onSelect={(id) =>
              setParams(
                id ? { ...params, stack: id } : { stack: id, services: [] },
              )
            }
            disabled={disabled}
          />
          <MultiSelect
            leftSection={<ICONS.Service size="1rem" />}
            placeholder={params.services?.length ? undefined : "All services"}
            value={params.services}
            data={allServices}
            onChange={(services) => setParams({ ...params, services })}
            styles={{ inputField: { width: 130 } }}
            searchable
            clearable
          />
        </Group>
      );
    },
  },
  StopStack: {
    params: { stack: "" },
    Component: ({ params, setParams, disabled }) => {
      const allServices = useRead("ListStackServices", {
        stack: params.stack,
      }).data?.map((s) => s.service);
      return (
        <Group>
          <ResourceSelector
            type="Stack"
            selected={params.stack}
            onSelect={(id) =>
              setParams(
                id ? { ...params, stack: id } : { stack: id, services: [] },
              )
            }
            disabled={disabled}
          />
          <MultiSelect
            leftSection={<ICONS.Service size="1rem" />}
            placeholder={params.services?.length ? undefined : "All services"}
            value={params.services}
            data={allServices}
            onChange={(services) => setParams({ ...params, services })}
            styles={{ inputField: { width: 130 } }}
            searchable
            clearable
          />
        </Group>
      );
    },
  },
  DestroyStack: {
    params: { stack: "" },
    Component: ({ params, setParams, disabled }) => (
      <ResourceSelector
        type="Stack"
        selected={params.stack}
        onSelect={(stack) => setParams({ stack })}
        disabled={disabled}
      />
    ),
  },
  BatchDestroyStack: {
    params: { pattern: "", tags: [] },
    Component: ({ params, setParams, disabled }) => (
      <Group>
        <TextUpdateModal
          title="Match stacks"
          value={
            params.pattern ||
            "# Match stacks by name, id, wildcard, or \\regex\\.\n"
          }
          onUpdate={(pattern) =>
            setParams({ ...params, pattern })
          }
          disabled={disabled}
          useMonaco
          monacoLanguage="string_list"
        />
        <TagMultiSelector
          title="Match Tags"
          value={params.tags ?? []}
          onChange={(tags) => setParams({ ...params, tags })}
          disabled={disabled}
          useName
        />
      </Group>
    ),
  },
  CloneRepo: {
    params: { repo: "" },
    Component: ({ params, setParams, disabled }) => (
      <ResourceSelector
        type="Repo"
        selected={params.repo}
        onSelect={(repo) => setParams({ repo })}
        disabled={disabled}
      />
    ),
  },
  BatchCloneRepo: {
    params: { pattern: "", tags: [] },
    Component: ({ params, setParams, disabled }) => (
      <Group>
        <TextUpdateModal
          title="Match repos"
          value={
            params.pattern ||
            "# Match repos by name, id, wildcard, or \\regex\\.\n"
          }
          onUpdate={(pattern) =>
            setParams({ ...params, pattern })
          }
          disabled={disabled}
          useMonaco
          monacoLanguage="string_list"
        />
        <TagMultiSelector
          title="Match Tags"
          value={params.tags ?? []}
          onChange={(tags) => setParams({ ...params, tags })}
          disabled={disabled}
          useName
        />
      </Group>
    ),
  },
  PullRepo: {
    params: { repo: "" },
    Component: ({ params, setParams, disabled }) => (
      <ResourceSelector
        type="Repo"
        selected={params.repo}
        onSelect={(repo) => setParams({ repo })}
        disabled={disabled}
      />
    ),
  },
  BatchPullRepo: {
    params: { pattern: "", tags: [] },
    Component: ({ params, setParams, disabled }) => (
      <Group>
        <TextUpdateModal
          title="Match repos"
          value={
            params.pattern ||
            "# Match repos by name, id, wildcard, or \\regex\\.\n"
          }
          onUpdate={(pattern) =>
            setParams({ ...params, pattern })
          }
          disabled={disabled}
          useMonaco
          monacoLanguage="string_list"
        />
        <TagMultiSelector
          title="Match Tags"
          value={params.tags ?? []}
          onChange={(tags) => setParams({ ...params, tags })}
          disabled={disabled}
          useName
        />
      </Group>
    ),
  },
  PruneImages: {
    params: { server: "" },
    Component: ({ params, setParams, disabled }) => (
      <ResourceSelector
        type="Server"
        selected={params.server}
        onSelect={(server) => setParams({ server })}
        disabled={disabled}
      />
    ),
  },
  PruneSystem: {
    params: { server: "" },
    Component: ({ params, setParams, disabled }) => (
      <ResourceSelector
        type="Server"
        selected={params.server}
        onSelect={(server) => setParams({ server })}
        disabled={disabled}
      />
    ),
  },
  RunSync: {
    params: { sync: "" },
    Component: ({ params, setParams, disabled }) => (
      <ResourceSelector
        type="ResourceSync"
        selected={params.sync}
        onSelect={(id) => setParams({ sync: id })}
        disabled={disabled}
      />
    ),
  },
  CommitSync: {
    params: { sync: "" },
    Component: ({ params, setParams, disabled }) => (
      <ResourceSelector
        type="ResourceSync"
        selected={params.sync}
        onSelect={(id) => setParams({ sync: id })}
        disabled={disabled}
      />
    ),
  },
  ClearRepoCache: {
    params: {},
    Component: () => <></>,
  },
  BackupCoreDatabase: {
    params: {},
    Component: () => <></>,
  },
  RotateAllServerKeys: {
    params: {},
    Component: () => <></>,
  },
  RotateCoreKeys: {
    params: {},
    Component: ({ params, setParams, disabled }) => (
      <Group
        style={{ cursor: !disabled ? "pointer" : undefined }}
        onClick={() => {
          if (!disabled) {
            setParams({ force: !params.force });
          }
        }}
      >
        Force:
        <Switch checked={params.force} disabled={disabled} />
      </Group>
    ),
  },
  Sleep: {
    params: { duration_ms: 0 },
    Component: ({ params, setParams, disabled }) => {
      const [internal, setInternal] = useState(
        params.duration_ms?.toString() ?? "",
      );
      useEffect(() => {
        setInternal(params.duration_ms?.toString() ?? "");
      }, [params.duration_ms]);
      const durationMs = Number(internal);
      return (
        <TextInput
          placeholder="Duration in milliseconds"
          value={internal}
          onChange={(e) => setInternal(e.target.value)}
          onBlur={() => {
            const duration_ms = Number(internal);
            if (duration_ms) {
              setParams({ duration_ms });
            } else {
              notifications.show({
                message: "Duration must be valid number",
                color: "red",
              });
            }
          }}
          disabled={disabled}
          error={!durationMs ? "Duration must be a valid number" : undefined}
        />
      );
    },
  },
};
