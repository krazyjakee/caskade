import { useLingui } from "@lingui/react/macro";
import { RefreshCw } from "lucide-react";
import { SchemaSettings } from "./SchemaSettings";

/** Game updates (`core.updates`), shown under the installer's settings. */
export function UpdateSettings() {
  const { t } = useLingui();
  return (
    <SchemaSettings
      namespace="core.updates"
      title={t`Game updates`}
      icon={RefreshCw}
      description={t`Games installed from GitHub are checked for new releases without an account. Updating keeps your saves and settings.`}
      resetLabel={t`Reset update settings`}
    />
  );
}
