import {
  usePermissions,
  useRead,
  useWebhookIdOrName,
  useWebhookIntegrations,
  useWrite,
} from "@/lib/hooks";
import {
  Config,
  ConfigGroupArgs,
  ConfigItem,
  ConfigList,
  ConfigProps,
  ShowHideButton,
} from "mogh_ui";
import { Group, Stack, Text } from "@mantine/core";
import { useLocalStorage } from "@mantine/hooks";
import { Types } from "komodo_client";
import ResourceLink from "@/resources/link";
import ResourceSelector from "@/resources/selector";
import SystemCommand from "@/components/config/system-command";
import { ProviderSelectorConfig } from "@/components/config/provider-selector";
import { AccountSelectorConfig } from "@/components/config/account-selector";
import LinkedRepo from "@/components/config/linked-repo";
import { useFullStack } from "..";
import { ReactNode } from "react";
import WebhookBuilder from "@/components/webhook/builder";
import CopyWebhookUrl from "@/components/webhook/copy-url";

export default function StackConfig({
  id,
  titleOther,
}: {
  id: string;
  titleOther: ReactNode;
}) {
  const [show, setShow] = useLocalStorage({
    key: `stack-${id}-show-v2`,
    defaultValue: {
      webhooks: true,
    },
  });
  const { canWrite } = usePermissions({ type: "Stack", id });
  const stack = useFullStack(id);
  const config = stack?.config;
  const name = stack?.name;
  const global_disabled =
    useRead("GetCoreInfo", {}).data?.ui_write_disabled ?? false;
  const [update, setUpdate] = useLocalStorage<Partial<Types.StackConfig>>({
    key: `stack-${id}-update-v1`,
    defaultValue: {},
  });
  const { mutateAsync } = useWrite("UpdateStack");
  const { getIntegration } = useWebhookIntegrations();
  const [idOrName] = useWebhookIdOrName();

  if (!config) return null;

  const disabled = global_disabled || !canWrite;

  const gitProvider = update.git_provider ?? config.git_provider;
  const webhookIntegration = getIntegration(gitProvider);
  const repoLinked = !!(update.linked_repo ?? config.linked_repo);
  const serverId = update.server_id || config.server_id;

  const server: ConfigGroupArgs<Types.StackConfig> = {
    label: "Server",
    labelHidden: true,
    fields: {
      server_id: (server_id, set) => (
        <ConfigItem
          label={
            server_id ? (
              <Group fz="h3" fw="bold">
                Server:
                <ResourceLink
                  type="Server"
                  id={server_id}
                  fz="h3"
                  iconSize="1.2rem"
                />
              </Group>
            ) : (
              "Select Server"
            )
          }
          description="Select the Server to deploy on."
        >
          <ResourceSelector
            type="Server"
            selected={server_id}
            onSelect={(server_id) => set({ server_id })}
            disabled={disabled}
            clearable
          />
        </ConfigItem>
      ),
    },
  };

  const source: ConfigGroupArgs<Types.StackConfig> = {
    label: "Source",
    labelHidden: true,
    fields: {
      linked_repo: (linkedRepo, set) => (
        <LinkedRepo
          linkedRepo={linkedRepo}
          repoLinked={repoLinked}
          set={set}
          disabled={disabled}
        />
      ),
      ...(!repoLinked
        ? {
            git_provider: (provider, set) => {
              const https = update.git_https ?? config.git_https;
              return (
                <ProviderSelectorConfig
                  accountType="git"
                  selected={provider}
                  disabled={disabled}
                  onSelect={(git_provider) => set({ git_provider })}
                  https={https}
                  onHttpsSwitch={() => set({ git_https: !https })}
                />
              );
            },
            git_account: (value, set) => (
              <AccountSelectorConfig
                id={serverId}
                type={serverId ? "Server" : "None"}
                accountType="git"
                provider={gitProvider}
                selected={value}
                onSelect={(git_account) => set({ git_account })}
                disabled={disabled}
                placeholder="None"
              />
            ),
            repo: {
              placeholder: "Enter repo",
              description:
                "The repo path on the provider. {namespace}/{repo_name}",
            },
            branch: {
              placeholder: "Enter branch",
              description: "Select a custom branch, or default to 'main'.",
            },
            commit: {
              label: "Commit Hash",
              placeholder: "Input commit hash",
              description:
                "Optional. Switch to a specific commit hash after cloning the branch.",
            },
            clone_path: {
              placeholder: "/clone/path/on/host",
              description: (
                <Stack gap="0">
                  <Text component="span">
                    Explicitly specify the folder on the host to clone the repo
                    in.
                  </Text>
                  <Text component="span">
                    If <b>relative</b> (no leading '/'), relative to{" "}
                    {"$root_directory/stacks/" + stack.name}
                  </Text>
                </Stack>
              ),
            },
          }
        : {}),
      reclone: {
        description:
          "Delete the repo folder and clone it again, instead of using 'git pull'.",
      },
    },
  };

  const units: ConfigGroupArgs<Types.StackConfig> = {
    label: "Units",
    labelHidden: true,
    fields: {
      run_directory: {
        label: "Run Directory",
        placeholder: "path/to/units",
        description: (
          <>
            The directory holding the quadlet unit files (.container, .pod,
            .network, .volume, ...), <b>relative to the root of the repo.</b>
          </>
        ),
      },
    },
  };

  const secrets: ConfigGroupArgs<Types.StackConfig> = {
    label: "Secrets",
    description:
      "Secret references in the unit files are resolved from the secret store collection at deploy time.",
    fields: {
      secrets_collection: {
        label: "Secrets Collection",
        placeholder: "Input collection name",
        description:
          "The secret store collection this Stack reads secrets from. Required if any unit file references a secret.",
      },
      skip_secret_interp: {
        label: "Skip Secret Interpolation",
        description:
          "Do not interpolate variables or secrets into the unit files.",
      },
    },
  };

  const links: ConfigGroupArgs<Types.StackConfig> = {
    label: "Links",
    labelHidden: true,
    fields: {
      links: (values, set) => (
        <ConfigList
          label="Links"
          addLabel="Add Link"
          description="Add quick links in the resource header"
          field="links"
          values={values ?? []}
          set={set}
          disabled={disabled}
          placeholder="Input link"
        />
      ),
    },
  };

  const webhooks: ConfigGroupArgs<Types.StackConfig> = {
    label: "Webhooks",
    description: `Copy the webhook given here, and configure your ${webhookIntegration}-style repo provider to send webhooks to Komodo`,
    actions: (
      <ShowHideButton
        show={show.webhooks}
        setShow={(webhooks) => setShow({ ...show, webhooks })}
      />
    ),
    contentHidden: !show.webhooks,
    fields: {
      ["Guard" as any]: () => {
        if (update.branch ?? config.branch) {
          return null;
        }
        return (
          <ConfigItem label="Configure Branch">
            <Text>Must configure Branch before webhooks will work.</Text>
          </ConfigItem>
        );
      },
      ["Builder" as any]: () => <WebhookBuilder gitProvider={gitProvider} />,
      ["Deploy" as any]: () =>
        (update.branch ?? config.branch) && (
          <CopyWebhookUrl
            label="Webhook URL - Deploy"
            integration={webhookIntegration}
            path={`/stack/${idOrName === "Id" ? id : encodeURIComponent(name ?? "...")}/deploy`}
          />
        ),
      webhook_force_deploy: {
        description:
          "Usually the Stack won't deploy unless there are changes to the files. Use this to force deploy.",
      },
      webhook_enabled: true,
      webhook_secret: {
        description:
          "Provide a custom webhook secret for this resource, or use the global default.",
        placeholder: "Input custom secret",
      },
    },
  };

  const advanced: ConfigGroupArgs<Types.StackConfig>[] = [
    {
      label: "Pre Deploy",
      labelHidden: true,
      fields: {
        pre_deploy: (value, set) => (
          <ConfigItem
            label="Pre Deploy"
            description="Execute a shell command before the units are deployed. The 'path' is relative to the Run Directory"
          >
            <SystemCommand
              value={value}
              set={(value) => set({ pre_deploy: value })}
              disabled={disabled}
            />
          </ConfigItem>
        ),
      },
    },
    {
      label: "Post Deploy",
      labelHidden: true,
      fields: {
        post_deploy: (value, set) => (
          <ConfigItem
            label="Post Deploy"
            description="Execute a shell command after the units are deployed. The 'path' is relative to the Run Directory"
          >
            <SystemCommand
              value={value}
              set={(value) => set({ post_deploy: value })}
              disabled={disabled}
            />
          </ConfigItem>
        ),
      },
    },
    {
      label: "Ignore Units",
      labelHidden: true,
      fields: {
        ignore_units: (values, set) => (
          <ConfigList
            label="Ignore Units"
            addLabel="Add Unit"
            description="Units listed here are ignored when checking the Stack state. Use this for init containers that exit early. Give the unit file name, eg 'init.container'."
            field="ignore_units"
            values={values ?? []}
            set={set}
            disabled={disabled}
            placeholder="init.container"
          />
        ),
      },
    },
    {
      label: "Pull Images",
      labelHidden: true,
      fields: {
        registry_provider: (provider, set) => (
          <ProviderSelectorConfig
            description="Login to a registry for private image access."
            accountType="docker"
            selected={provider}
            disabled={disabled}
            onSelect={(registry_provider) => set({ registry_provider })}
          />
        ),
        registry_account: (value, set) => {
          const provider = update.registry_provider ?? config.registry_provider;
          if (!provider) {
            return null;
          }
          return (
            <AccountSelectorConfig
              id={serverId}
              type={serverId ? "Server" : "None"}
              accountType="docker"
              provider={provider}
              selected={value}
              onSelect={(registry_account) => set({ registry_account })}
              disabled={disabled}
              placeholder="None"
            />
          );
        },
        auto_pull: {
          label: "Pre Pull Images",
          description:
            "Run 'podman pull' for every container unit before restarting the units.",
        },
      },
    },
    {
      label: "Alerts",
      labelHidden: true,
      fields: {
        send_alerts: {
          description: "Send StackStateChange alerts for this Stack.",
        },
      },
    },
  ];

  const groups: ConfigProps<Types.StackConfig>["groups"] = {
    "": [server, source, units, secrets, links, webhooks],
    advanced,
  };

  return (
    <Config
      titleOther={titleOther}
      disabled={disabled}
      original={config}
      update={update}
      setUpdate={setUpdate}
      onSave={async () => {
        await mutateAsync({ id, config: update });
      }}
      groups={groups}
    />
  );
}
