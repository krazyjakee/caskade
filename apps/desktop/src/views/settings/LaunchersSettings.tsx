import { Button, SettingsSection, StatusDot, Switch } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { Link, useRouter } from "@tanstack/react-router";
import { Gamepad2, RefreshCw } from "lucide-react";
import { useEffect, useRef } from "react";
import { scanning } from "../../api/discovery";
import { formatPath } from "../../api/presentation";
import { FailureNotice } from "../../components/FailureNotice";
import { useDiscovery } from "./discovery/DiscoveryProvider";
import { detectedLaunchers, launcherEnabled, withLauncher } from "./launchers";
import { ResetDefaults } from "./ResetDefaults";
import { SettingsFeedback, SettingsLoading, useSettingsSection } from "./useSettings";

/** The launchers discovery looks for on this device, whether it found each,
 * and a switch for scanning it (#48). The one place `core.launchers` is
 * changed; Library Discovery links here. */
export function LaunchersSettings() {
  const { t } = useLingui();
  const { settings, setSettings, saving, error, reload } = useSettingsSection<{
    enabled: Record<string, boolean>;
  }>("core.launchers");
  const discovery = useDiscovery();
  const snapshot = discovery.data;
  const detect = discovery.action.mutate;
  // Detection comes with each scan; a device that hasn't scanned since
  // Caskade started looks once when the page opens.
  const asked = useRef(false);
  useEffect(() => {
    if (!snapshot || asked.current || scanning(snapshot) || snapshot.launchers.length > 0) return;
    asked.current = true;
    detect({ type: "launchers" });
  }, [snapshot, detect]);
  if (!settings) return <SettingsLoading error={error} reload={reload} />;
  const launchers = detectedLaunchers(snapshot?.launchers ?? []);
  const looking = discovery.isPending || discovery.action.isPending;

  return (
    <div data-settings="launchers" className="space-y-4">
      <div className="flex items-center justify-between gap-3">
        <SettingsFeedback error={error} saving={saving} />
        <ResetDefaults
          group={t({ message: "Launchers", context: "settings tab" })}
          label={t`Reset launcher settings`}
          parts={[
            {
              namespace: "core.launchers",
              whole: true,
              fields: [{ key: "enabled", label: t`Which launchers are scanned` }],
            },
          ]}
          isDisabled={saving}
        />
      </div>
      <SettingsSection
        icon={Gamepad2}
        title={t`Launchers`}
        description={t`Games from the launchers turned on here join your library. Turning one off removes the games only it found at the next scan.`}
        aside={
          <Button
            size="sm"
            variant="ghost"
            icon={RefreshCw}
            isDisabled={looking || scanning(snapshot)}
            onPress={() => detect({ type: "launchers" })}
          >
            <Trans>Look again</Trans>
          </Button>
        }
      >
        <DiscoveryLink />
        {discovery.error || discovery.action.error ? (
          <FailureNotice
            className="mt-3"
            error={discovery.error ?? discovery.action.error}
            onRetry={() =>
              discovery.error ? void discovery.refetch() : detect({ type: "launchers" })
            }
          />
        ) : launchers.length === 0 ? (
          <p role="status" className="mt-3 text-sm text-fg-muted">
            {looking || !snapshot || scanning(snapshot) ? (
              <Trans>Looking for launchers on this device…</Trans>
            ) : (
              <Trans>Caskade knows no launchers for this device's system.</Trans>
            )}
          </p>
        ) : (
          <ul className="mt-1 flex flex-col">
            {launchers.map((launcher) => {
              const on = launcherEnabled(launcher, settings.enabled);
              const found = launcher.found.length > 0;
              return (
                <li
                  key={launcher.id}
                  data-part="launcher"
                  data-status={found ? "detected" : "not-found"}
                  className="flex items-center gap-3 border-t border-border py-2 first:border-t-0"
                >
                  <div className="min-w-0 flex-1">
                    <p className="text-sm font-medium">{launcher.name}</p>
                    {found ? (
                      launcher.found.map((row) => (
                        <p
                          key={`${row.source}:${row.path}:${row.version}`}
                          className="break-all font-mono text-xs text-fg-muted"
                        >
                          {row.path ? formatPath(row.path) : t`Location unknown`}
                          {row.version ? ` · ${row.version}` : ""}
                        </p>
                      ))
                    ) : (
                      <p className="text-xs text-fg-muted">
                        <Trans>Not found on this device</Trans>
                      </p>
                    )}
                  </div>
                  <StatusDot tone={on && found ? "ok" : "off"}>
                    {on ? t`Scanned` : t`Not scanned`}
                  </StatusDot>
                  <Switch
                    size="sm"
                    aria-label={launcher.name}
                    isSelected={on}
                    onChange={(next) =>
                      setSettings((current) => ({
                        ...current,
                        enabled: withLauncher(current.enabled, launcher, next),
                      }))
                    }
                  />
                </li>
              );
            })}
          </ul>
        )}
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
