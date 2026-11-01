/** Settings → Scripts (#180): the global lifecycle scripts, which run with
 * every game's launches. Admins only, as the core requires. */
import { SettingsSection } from "@caskade/ui";
import { useLingui } from "@lingui/react/macro";
import { TerminalSquare } from "lucide-react";
import { ScriptsPanel } from "../../components/scripts/ScriptsPanel";

export function ScriptsSettings() {
  const { t } = useLingui();
  return (
    <div data-settings="scripts" className="flex flex-col gap-4">
      <SettingsSection
        icon={TerminalSquare}
        title={t`Global scripts`}
        description={t`Commands that run around every game's launch. Emulator profiles and games can add their own; see the order at launch below.`}
      >
        <ScriptsPanel scope={{ type: "global" }} />
      </SettingsSection>
    </div>
  );
}
