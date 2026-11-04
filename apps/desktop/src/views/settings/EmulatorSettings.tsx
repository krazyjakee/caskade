import { SettingRow, SettingsSection, Switch } from "@caskade/ui";
import { useLingui } from "@lingui/react/macro";
import { getRouteApi } from "@tanstack/react-router";
import { Maximize, MonitorPlay } from "lucide-react";
import { EmulationPanel, type EmulationSection, sections } from "./emulators/EmulationPanel";
import { RommPanel } from "./romm/RommPanel";
import { SettingsStatus, useSettingsSection } from "./useSettings";

const settingsRoute = getRouteApi("/settings");

/** `core.emulation`: console mode always plays fullscreen; this makes desktop mode do it too. */
function DesktopFullscreen() {
  const { t } = useLingui();
  const section = useSettingsSection<{ desktopFullscreen: boolean }>("core.emulation");
  const { settings, setSettings } = section;
  // A setting in a card of its own, not among the overview's counts (#54).
  return (
    <SettingsSection
      icon={MonitorPlay}
      title={t`Playing`}
      description={t`How emulated games open.`}
    >
      <SettingsStatus section={section} what={t`Playing`} />
      {settings && (
        <SettingRow
          icon={Maximize}
          label={t`Desktop full screen`}
          description={t`Play emulated games fullscreen in desktop mode too. Console mode always does.`}
        >
          <Switch
            isSelected={settings.desktopFullscreen}
            onChange={(desktopFullscreen) => setSettings((old) => ({ ...old, desktopFullscreen }))}
          />
        </SettingRow>
      )}
    </SettingsSection>
  );
}

/** Settings → Emulation (EMULATION-PLAN §7). The section lives in the URL as `?section=`. */
export function EmulatorSettings() {
  const search = settingsRoute.useSearch();
  const navigate = settingsRoute.useNavigate();
  const section: EmulationSection =
    sections.find((row) => row.id === search.section)?.id ?? "emulators";
  const setSection = (id: EmulationSection) =>
    navigate({ search: (old) => ({ ...old, section: id }), replace: true });
  return (
    <EmulationPanel
      section={section}
      setSection={setSection}
      header={<DesktopFullscreen />}
      renderExtra={() => <RommPanel />}
    />
  );
}
