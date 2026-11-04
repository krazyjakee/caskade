import { Button, SettingsSection, StatusDot, Switch } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { Link, useRouter } from "@tanstack/react-router";
import { Gamepad2, RotateCcw } from "lucide-react";
import { launcherSources } from "../../mocks/settings";
import { SettingsFeedback, SettingsLoading, useSettingsSection } from "./useSettings";

export function LaunchersSettings() {
  const { t } = useLingui();
  const { settings, setSettings, saving, error, reload, reset } = useSettingsSection<{
    enabled: Record<string, boolean>;
  }>("core.launchers");
  if (!settings) return <SettingsLoading error={error} reload={reload} />;
  const launchers = launcherSources.map((launcher) => ({
    ...launcher,
    enabled: settings.enabled[launcher.id] ?? true,
  }));

  return (
    <div data-settings="launchers" className="grid items-start gap-4 xl:grid-cols-2">
      <div className="col-span-full">
        <SettingsFeedback error={error} saving={saving} />
      </div>
      <SettingsSection
        icon={Gamepad2}
        title={t`Launcher Sources`}
        description={t`Detect games from your installed launchers. Turning one off removes the games only it found at the next scan.`}
      >
        <DiscoveryLink />
        <ul className="mt-1 flex flex-col">
          {launchers.map((launcher) => (
            <li
              key={launcher.id}
              data-part="launcher"
              data-status={launcher.status}
              className="flex items-center gap-3 border-t border-border py-2 first:border-t-0"
            >
              <span
                aria-hidden
                className="grid size-6 shrink-0 place-items-center rounded-[5px] font-display text-xs font-bold text-white"
                style={{
                  background: `linear-gradient(135deg, hsl(${launcher.hue} 70% 55%), hsl(${launcher.hue} 70% 35%))`,
                }}
              >
                {launcher.name[0]}
              </span>
              <span className="max-w-md min-w-0 flex-1 truncate text-sm">{launcher.name}</span>
              <StatusDot tone={launcher.enabled ? "ok" : "off"}>
                {launcher.enabled ? t`Enabled for scans` : t`Disabled for scans`}
              </StatusDot>
              <Switch
                size="sm"
                aria-label={launcher.name}
                isSelected={launcher.enabled}
                onChange={(enabled) =>
                  setSettings((current) => ({
                    ...current,
                    enabled: { ...current.enabled, [launcher.id]: enabled },
                  }))
                }
              />
            </li>
          ))}
        </ul>
        <Button
          variant="ghost"
          size="sm"
          icon={RotateCcw}
          className="mt-2 w-full"
          onPress={reset}
          isDisabled={saving}
        >
          <Trans>Reset launcher preferences</Trans>
        </Button>
      </SettingsSection>
    </div>
  );
}

/** Scans, folders and the operating system's checks are Library Discovery's
 * (#46); it links back here. Without a router (a panel on its own) there is
 * nothing to link through. */
function DiscoveryLink() {
  const router = useRouter({ warn: false }) as ReturnType<typeof useRouter> | undefined;
  if (!router) return null;
  return (
    <p className="mt-1 text-sm text-fg-muted">
      <Trans>
        How and when games are scanned:{" "}
        <Link to="/settings" search={{ tab: "library-discovery" }} className="text-accent-alt">
          Library Discovery
        </Link>
      </Trans>
    </p>
  );
}
