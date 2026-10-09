import { useSettingsView, useUser } from "@/lib/hooks";
import { Stack } from "@mantine/core";
import SettingsCoreInfo from "./core-info";
import { MobileFriendlyTabs } from "mogh_ui";
import SettingsTags from "./tags";
import SettingsProviders from "./providers";
import { ICONS } from "@/lib/icons";
import SettingsOnboardingKeys from "./onboarding";
import SettingsUsers from "./users";

export default function Settings() {
  const user = useUser().data;
  const [view, setView] = useSettingsView();
  return (
    <Stack gap="xl" mb="50vh">
      <SettingsCoreInfo />
      <MobileFriendlyTabs
        value={view}
        onValueChange={setView as any}
        tabsProps={{ color: "green" }}
        tabs={[
          {
            value: "Tags",
            content: <SettingsTags />,
            icon: ICONS.Tag,
          },
          {
            value: "Providers",
            content: <SettingsProviders />,
            icon: ICONS.Provider,
          },
          {
            value: "Users",
            content: <SettingsUsers />,
            icon: ICONS.User,
            hidden: !user?.admin,
          },
          {
            value: "Onboarding",
            content: <SettingsOnboardingKeys />,
            icon: ICONS.OnboardingKey,
          },
        ]}
      />
    </Stack>
  );
}
