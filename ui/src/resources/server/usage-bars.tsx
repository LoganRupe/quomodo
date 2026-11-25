import { Group, Progress, Stack, Text, Tooltip } from "@mantine/core";
import { Types } from "komodo_client";
import { LucideIcon } from "lucide-react";
import { ICONS } from "@/lib/icons";
import {
  USAGE_MISSING,
  UsageKind,
  fmtBytes,
  fmtCores,
  hostSize,
  usageValue,
} from "@/components/usage";

/** Container CPU and memory for a dashboard card, laid out like the
 * host stats in `ServerStatsCard`. The bars are green and show the
 * share of the host `server` runs on. */
export default function UsageBars({
  usage,
  server,
}: {
  usage: Types.ContainerUsage | undefined;
  /** Any agent on the host, to size the bars. */
  server: Types.ServerListItem | undefined;
}) {
  const host = hostSize(server);
  return (
    <Stack gap="xs">
      <UsageItem
        icon={ICONS.Cpu}
        label="CPU"
        usage={usage}
        kind="cpu"
        of={host.cores}
      />
      <UsageItem
        icon={ICONS.Memory}
        label="Memory"
        usage={usage}
        kind="mem"
        of={host.memBytes}
      />
    </Stack>
  );
}

function UsageItem({
  icon: Icon,
  label,
  usage,
  kind,
  of,
}: {
  icon: LucideIcon;
  label: string;
  usage: Types.ContainerUsage | undefined;
  kind: UsageKind;
  of: number | undefined;
}) {
  const value = usageValue(usage, kind);
  const share =
    value !== undefined && of ? Math.min(100, (value / of) * 100) : 0;
  return (
    <Group gap="xs" wrap="nowrap" c="dimmed">
      <Icon size={13} />
      <Stack gap="0.1rem" w="calc(100% - 20px)">
        <Group justify="space-between">
          <Text size="sm">{label}</Text>
          {value === undefined ? (
            <Tooltip label={USAGE_MISSING} w={320} multiline>
              <Text size="sm">–</Text>
            </Tooltip>
          ) : (
            <Text size="sm" c="var(--mantine-color-text)">
              {kind === "cpu" ? fmtCores(value, true) : fmtBytes(value)}
            </Text>
          )}
        </Group>
        <Progress color="green" value={share} size={4} />
      </Stack>
    </Group>
  );
}
