import { SegmentedControl, Group } from "@mantine/core";
import { Rows3, Table } from "lucide-react";
import { StackView, useStackView } from "@/lib/hooks";

const VIEWS: { value: StackView; label: string; Icon: typeof Table }[] = [
  { value: "table", label: "Table", Icon: Table },
  { value: "hosts", label: "Host table", Icon: Rows3 },
];

/** Switches the Stacks list between one row per stack
 * and stacks grouped under their host. */
export default function StackViewSelect() {
  const [view, setView] = useStackView();
  return (
    <SegmentedControl
      value={view}
      onChange={(value) => setView(value as StackView)}
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
