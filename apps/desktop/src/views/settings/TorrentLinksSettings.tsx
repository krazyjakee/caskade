import { Button, SettingRow, SettingsSection, Switch } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { ExternalLink, Magnet } from "lucide-react";
import {
  type TorrentLinks,
  useChooseTorrentLinks,
  useSetTorrentLinks,
  useTorrentLinks,
} from "../../api/torrentLinks";

/** Linux: Caskade takes them from the torrent client and hands them back. */
function DirectRow({ status }: { status: TorrentLinks }) {
  const { t } = useLingui();
  const set = useSetTorrentLinks();
  return (
    <>
      <SettingRow
        icon={Magnet}
        label={t`Open them with Caskade`}
        description={t`Your browser and file manager send magnet links and .torrent files to Caskade instead of your torrent client. Turning this off gives them back.`}
      >
        <Switch
          isSelected={status.magnet || status.torrentFiles}
          isDisabled={set.isPending}
          onChange={(open) => set.mutate(open)}
        />
      </SettingRow>
      {set.error && (
        <p role="alert" className="mt-3 text-sm text-danger">
          {set.error.message}
        </p>
      )}
    </>
  );
}

/** Windows: Caskade is offered, and the user chooses it in Default apps. */
function ChooserRow({ status }: { status: TorrentLinks }) {
  const { t } = useLingui();
  const set = useSetTorrentLinks();
  const choose = useChooseTorrentLinks();
  const chosen = status.magnet && status.torrentFiles;
  return (
    <>
      <SettingRow
        icon={Magnet}
        label={t`Offer Caskade for them`}
        description={
          !status.registered
            ? t`Windows lists Caskade in Default apps, where you choose it for magnet links and .torrent files.`
            : chosen
              ? t`Windows opens magnet links and .torrent files with Caskade.`
              : t`Choose Caskade for magnet links and .torrent files in Windows' Default apps.`
        }
      >
        <Switch
          isSelected={status.registered}
          isDisabled={set.isPending}
          onChange={(open) => set.mutate(open)}
        />
      </SettingRow>
      {status.registered && !chosen && (
        <div className="mt-2 flex justify-end">
          <Button onPress={() => choose.mutate()} isDisabled={choose.isPending}>
            <ExternalLink className="size-4" />
            <Trans>Open Default apps</Trans>
          </Button>
        </div>
      )}
      {set.error && (
        <p role="alert" className="mt-3 text-sm text-danger">
          {set.error.message}
        </p>
      )}
      {choose.error && (
        <p role="alert" className="mt-3 text-sm text-danger">
          {choose.error.message}
        </p>
      )}
    </>
  );
}

/** Magnet links and .torrent files (#159): how to add one, and whether the
 * OS opens them with Caskade (Settings → Installer). */
export function TorrentLinksSettings() {
  const { t } = useLingui();
  const { data: status } = useTorrentLinks();
  return (
    <SettingsSection
      icon={Magnet}
      title={t`Magnet links and torrent files`}
      description={t`Paste a magnet link anywhere in Caskade, drop a .torrent file on it, or use Add game. Nothing downloads until you confirm, and nothing is seeded afterwards.`}
    >
      {status?.support === "direct" && <DirectRow status={status} />}
      {status?.support === "chooser" && <ChooserRow status={status} />}
      {status?.support === "unsupported" && (
        <p className="text-sm text-fg-muted">
          <Trans>This system can't send magnet links and .torrent files to Caskade yet.</Trans>
        </p>
      )}
    </SettingsSection>
  );
}
