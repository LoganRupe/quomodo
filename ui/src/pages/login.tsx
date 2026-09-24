import { LoginPage } from "mogh_ui";
import { useUserInvalidate } from "@/lib/hooks";

export default function Login(props: {
  passkeyIsPending?: boolean;
  totpIsPending?: boolean;
}) {
  const userInvalidate = useUserInvalidate();
  return (
    <LoginPage
      {...props}
      appName="QUOMODO"
      iconLink="/quomodo-app-icon.svg"
      iconLinkAlt="Quomodo"
      exampleConfigLink="https://github.com/LoganRupe/quomodo/blob/main/config/core.config.toml"
      onLogin={userInvalidate}
    />
  );
}
