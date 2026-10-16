import { SegmentedControl, Group } from "@mantine/core";
import { LayoutGrid, Rows3, Server } from "lucide-react";
import { ServerView, useServerView } from "@/lib/hooks";

const VIEWS: { value: ServerView; label: string; Icon: typeof Server }[] = [
  { value: "hosts", label: "Host table", Icon: Rows3 },
  { value: "cards", label: "Host cards", Icon: LayoutGrid },
  { value: "agents", label: "Agents", Icon: Server },
];

/** Switches the Servers list between grouping agents by host
 * (as a table or as cards) and one row per agent. */
export default function ServerViewSelect() {
  const [view, setView] = useServerView();
  return (
    <SegmentedControl
      value={view}
      onChange={(value) => setView(value as ServerView)}
      w={{ base: "100%", xs: "fit-content" }}
      data={VIEWS.map(({ value, label, Icon }) => ({
        value,
        label: (
          <Group gap="xs" wrap="nowrap" justify="center">
            <Icon size="1rem" />
            {label}
          </Group>
        ),
      }))}
    />
  );
}
