/** Settings → Store accounts' achievements (`core.achievements`, #185/#190):
 * whether to read them, refresh them in the background, and notify unlocks
 * that come in after a set's first fetch. */
import { SettingRow, SettingsSection, Switch } from "@caskade/ui";
import { useLingui } from "@lingui/react/macro";
import { Trophy } from "lucide-react";
import { useSettingsSection } from "../../views/settings/useSettings";

interface Settings {
  enabled: boolean;
  background: boolean;
  notifyUnlocks: boolean;
}

export function AchievementSettings() {
  const { t } = useLingui();
  const section = useSettingsSection<Settings>("core.achievements");
  const settings = section.settings;
  if (!settings) return null;
  const set = (change: Partial<Settings>) =>
    section.setSettings((current) => ({ ...current, ...change }));
  return (
    <SettingsSection
      icon={Trophy}
      title={t`Achievements`}
      description={t`Game pages show the achievements of connected accounts. Caskade only reads them; nothing is unlocked or changed at a store.`}
    >
      <SettingRow label={t`Show achievements`}>
        <Switch isSelected={settings.enabled} onChange={(enabled) => set({ enabled })} />
      </SettingRow>
      <SettingRow
        label={t`Refresh in the background`}
        description={t`After a game exits, and now and then for games played lately.`}
      >
        <Switch
          isSelected={settings.background}
          isDisabled={!settings.enabled}
          onChange={(background) => set({ background })}
        />
      </SettingRow>
      <SettingRow
        label={t`Notify new unlocks`}
        description={t`A notification for achievements unlocked since the last refresh. The first refresh of a game brings its history in without notifications.`}
      >
        <Switch
          isSelected={settings.notifyUnlocks}
          isDisabled={!settings.enabled}
          onChange={(notifyUnlocks) => set({ notifyUnlocks })}
        />
      </SettingRow>
    </SettingsSection>
  );
}
