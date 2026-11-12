/** A game's owned releases (#170): which store account owns it, and
 * installing it through that store's launcher. Installing is done when
 * Library Discovery finds the copy; until then the release says it waits. */
import { Button } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { Download } from "lucide-react";
import { formatDateTime } from "../../api/presentation";
import {
  type OwnedRelease,
  useOwnedReleases,
  useStoreAccountAction,
} from "../../api/storeAccounts";
import { FailureNotice } from "../../components/FailureNotice";

const systemNames: Record<string, string> = { windows: "Windows", mac: "macOS", linux: "Linux" };

function useDescribe() {
  const { t } = useLingui();
  return (release: OwnedRelease) => {
    const state = release.state;
    switch (state.type) {
      case "installed":
        return t`Installed.`;
      case "available":
        return null;
      case "installing":
        return t`Waiting for ${state.launcher} to finish installing it.`;
      case "launcherMissing":
        return t`${state.launcher} isn't installed on this computer.`;
      case "unsupportedSystem": {
        const systems = state.systems.map((s) => systemNames[s] ?? s).join(", ");
        return t`Runs on ${systems}, not on this computer's system.`;
      }
      case "unavailable":
        return state.reason === "noLongerOwned"
          ? t`The account no longer owns it.`
          : state.reason === "disconnected"
            ? t`The account is disconnected. Connect it again in Settings → Store accounts.`
            : t`This store isn't available in this version of Caskade.`;
      case "notSupported":
        return t`The store can't install games from Caskade.`;
    }
  };
}

export function OwnedReleases({
  gameId,
  status,
  primary,
}: {
  gameId: string;
  status: string;
  /** Nothing is installed: Install is the page's main action. */
  primary: boolean;
}) {
  const { t } = useLingui();
  const describe = useDescribe();
  const { data } = useOwnedReleases(gameId, status);
  const install = useStoreAccountAction((api, entryId: string) => api.install(entryId));
  const cancel = useStoreAccountAction((api, entryId: string) => api.cancelInstall(entryId));
  if (!data || data.length === 0) return null;
  return (
    <div data-part="owned" className="flex flex-col gap-2">
      {data.map((release, index) => {
        const note = describe(release);
        // The launcher's download is its own (#131).
        const launcher = release.state.type === "installing" ? release.state.launcher : null;
        const provider = release.providerName;
        const account = release.accountName;
        const refreshed =
          release.refreshedAtMs === null ? null : formatDateTime(new Date(release.refreshedAtMs));
        return (
          <div
            key={release.entryId}
            data-part="owned-release"
            data-status={release.state.type}
            className="flex flex-col gap-1.5 rounded-control border border-border p-3"
          >
            <span className="text-sm">
              <Trans>
                Owned on {provider} ({account})
              </Trans>
            </span>
            {refreshed && (
              <span className="text-xs text-fg-subtle">
                <Trans>Library refreshed {refreshed}.</Trans>
              </span>
            )}
            {note && <span className="text-xs text-fg-muted">{note}</span>}
            {launcher && (
              <span data-part="pause-elsewhere" className="text-xs text-fg-muted">
                {t({
                  message: `To pause or cancel it, use ${launcher}: Caskade can't pause a store's own download.`,
                  comment:
                    "A store's launcher (Steam, GOG Galaxy, the Epic Games Launcher) is installing a game",
                })}
              </span>
            )}
            {release.state.type === "available" && (
              <Button
                variant={primary && index === 0 ? "primary" : "secondary"}
                data-focus-default={(primary && index === 0) || undefined}
                isDisabled={install.isPending}
                onPress={() => install.mutate(release.entryId)}
              >
                <Download className="size-4" />
                {t`Install with ${release.state.launcher}`}
              </Button>
            )}
            {release.state.type === "installing" && (
              <Button size="sm" variant="ghost" onPress={() => cancel.mutate(release.entryId)}>
                <Trans>Stop waiting</Trans>
              </Button>
            )}
          </div>
        );
      })}
      {install.error && <FailureNotice variant="inline" error={install.error} />}
    </div>
  );
}
