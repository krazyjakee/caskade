import { useLingui } from "@lingui/react/macro";
import { PackageOpen } from "lucide-react";
import { SchemaSettings } from "./SchemaSettings";

/** Controls, choices, labels and limits come from core's JSON Schema. */
export function InstallerSettings() {
  const { t } = useLingui();
  return (
    <SchemaSettings
      namespace="core.installer"
      title={t`Installer`}
      icon={PackageOpen}
      description={t`Preferences apply to the next install job. Jobs already running keep their current settings.`}
      resetLabel={t`Reset installer settings`}
    />
  );
}
