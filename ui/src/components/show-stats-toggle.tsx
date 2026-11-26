import { Button } from "@mantine/core";
import { Eye, EyeOff } from "lucide-react";
import { useDashboardPreferences } from "@/lib/hooks";

/** Show / Hide button for one of the container usage preferences,
 * styled like Servers' "Show Server Stats". */
export default function ShowStatsToggle({
  preference,
  label,
}: {
  preference: "showStackStats" | "showStackPageStats";
  /** Eg "Stack Stats" gives "Show Stack Stats" / "Hide Stack Stats". */
  label: string;
}) {
  const { preferences, togglePreference } = useDashboardPreferences();
  const on = !!preferences[preference];
  return (
    <Button
      variant="outline"
      onClick={() => togglePreference(preference)}
      leftSection={on ? <EyeOff size="1rem" /> : <Eye size="1rem" />}
      w={{ base: "100%", xs: "fit-content" }}
    >
      {on ? `Hide ${label}` : `Show ${label}`}
    </Button>
  );
}
