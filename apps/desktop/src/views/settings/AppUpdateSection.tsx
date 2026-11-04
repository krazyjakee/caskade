import { Button, SettingRow, SettingsSection, Switch } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { CircleArrowUp, Download, RefreshCw, RotateCw } from "lucide-react";
import {
  type AppUpdateStatus,
  canInstall,
  useAppUpdate,
  useAppUpdateAction,
} from "../../api/appUpdate";
import { formatDateTime, formatSize } from "../../api/presentation";
import { FailureNotice } from "../../components/FailureNotice";
import { ProgressLine } from "../activity/ProgressLine";
import { SettingsStatus, useSettingsSection } from "./useSettings";

/** What the status line under the version says. */
function useDescribe() {
  const { t } = useLingui();
  return (status: AppUpdateStatus) => {
    if (status.unavailable) return status.unavailable;
    const version = status.release?.version;
    switch (status.phase) {
      case "idle":
        return t`Not checked yet.`;
      case "checking":
        return t`Checking for updates…`;
      case "upToDate":
        return status.checkedAt === null
          ? t`Caskade is up to date.`
          : t`Caskade is up to date. Checked ${{ time: formatDateTime(status.checkedAt) }}.`;
      case "available":
        return t`Version ${{ version }} is available.`;
      case "downloading":
        return t`Downloading version ${{ version }}…`;
      case "installing":
        return t`Installing version ${{ version }}…`;
      case "ready":
        return t`Version ${{ version }} is installed. Restart Caskade to use it.`;
      case "failed":
        return status.error ?? t`The update failed.`;
    }
  };
}

/** Caskade's own updates (`core.appUpdates`): the version running, what the
 * last check found, and installing it. */
export function AppUpdateSection() {
  const { t } = useLingui();
  const describe = useDescribe();
  const query = useAppUpdate();
  const status = query.data;
  const action = useAppUpdateAction();
  const preferences = useSettingsSection<{ check: boolean }>("core.appUpdates");
  const title = t`Caskade updates`;
  const description = t`New versions of Caskade itself. Updates are signed, and nothing installs until you ask.`;
  // A failed status says so, with Try again, rather than leaving a gap (#55).
  if (!status)
    return query.isError ? (
      <SettingsSection icon={CircleArrowUp} title={title} description={description}>
        <FailureNotice
          variant="inline"
          className="mt-3"
          error={query.error}
          title={t`Caskade couldn't check its version`}
          onRetry={() => void query.refetch()}
        />
      </SettingsSection>
    ) : null;
  const busy =
    action.isPending ||
    status.phase === "checking" ||
    status.phase === "downloading" ||
    status.phase === "installing";
  const working = status.phase === "downloading" || status.phase === "installing";
  const notes = status.release?.notes;
  const version = status.release?.version;

  return (
    <SettingsSection icon={CircleArrowUp} title={title} description={description}>
      <SettingRow
        icon={RefreshCw}
        label={t`Caskade v${{ version: status.currentVersion }}`}
        description={describe(status)}
      >
        {status.phase === "ready" ? (
          <Button variant="primary" onPress={() => action.mutate("restart")} isDisabled={busy}>
            <RotateCw className="size-4" />
            <Trans>Restart now</Trans>
          </Button>
        ) : canInstall(status) ? (
          <Button variant="primary" onPress={() => action.mutate("install")} isDisabled={busy}>
            <Download className="size-4" />
            {status.phase === "failed" ? (
              <Trans>Try again</Trans>
            ) : (
              <Trans>Download and install</Trans>
            )}
          </Button>
        ) : (
          <Button
            onPress={() => action.mutate("check")}
            isDisabled={busy || status.unavailable !== null}
          >
            {status.phase === "checking" ? <Trans>Checking…</Trans> : <Trans>Check now</Trans>}
          </Button>
        )}
      </SettingRow>
      {working && (
        <ProgressLine
          label={t`Update download`}
          status={status.phase === "installing" ? t`Installing…` : t`Downloading…`}
          done={status.downloaded}
          total={status.total ?? 0}
          pending={formatSize(status.downloaded)}
        />
      )}
      {action.error && (
        <p role="alert" className="mt-3 text-sm text-danger">
          {action.error.message}
        </p>
      )}
      {notes && status.phase !== "upToDate" && (
        <div className="mt-3 flex flex-col gap-1.5">
          <span className="text-sm font-medium">
            <Trans>What's new in {version}</Trans>
          </span>
          <p className="max-h-48 overflow-y-auto whitespace-pre-line rounded-control border border-border bg-surface-base/30 px-3 py-2 text-xs text-fg-muted">
            {notes}
          </p>
        </div>
      )}
      {preferences.settings && status.unavailable === null && (
        <SettingRow
          icon={RefreshCw}
          label={t`Check automatically`}
          description={t`Look for a new version when Caskade starts and every six hours.`}
        >
          <Switch
            isSelected={preferences.settings.check}
            onChange={(check) => preferences.setSettings((current) => ({ ...current, check }))}
          />
        </SettingRow>
      )}
      <SettingsStatus section={preferences} what={t`Check automatically`} />
    </SettingsSection>
  );
}
