import { Trans, useLingui } from "@lingui/react/macro";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useRouter } from "@tanstack/react-router";
import { libraryKeys, useLibraryApi } from "../../api/provider";
import { backTarget, useGoBack } from "../../views/game/origin";
import { ConfirmDialog } from "../ConfirmDialog";

/** Asks before a game's menu removes a game Caskade installed or handed
 * off, or a web game (with its offline copy and saves): no scan brings
 * either back, so it goes for good. On its own page, the page goes back to
 * where it was opened from. */
export function RemoveGameDialog({
  gameId,
  title,
  web = false,
  uninstalls = false,
  keepsGame = false,
  close,
}: {
  gameId: string;
  title: string;
  /** Removes the game's web version. */
  web?: boolean;
  /** It has files Caskade installed, which are deleted first. */
  uninstalls?: boolean;
  /** A web game that has other entries, which stay. */
  keepsGame?: boolean;
  close: () => void;
}) {
  const { t } = useLingui();
  const api = useLibraryApi();
  const client = useQueryClient();
  const router = useRouter();
  const goBack = useGoBack();
  const remove = useMutation({
    mutationFn: async () => {
      if (web) {
        if (!api.removeWebGame) throw new Error(t`Web games aren't available here`);
        await api.removeWebGame(gameId);
        return;
      }
      if (!api.removeGame) throw new Error(t`Removing games isn't available here`);
      await api.removeGame(gameId);
    },
    onSuccess: () => {
      if (!web) void client.invalidateQueries({ queryKey: ["storage"] });
      void client.invalidateQueries({ queryKey: libraryKeys.all });
      close();
      const { pathname, search } = router.state.location;
      if (!keepsGame && pathname === `/games/${encodeURIComponent(gameId)}`)
        goBack(
          backTarget(gameId, (search as { from?: unknown }).from),
          web ? "/library?filter=web" : undefined,
        );
    },
  });
  return (
    <ConfirmDialog
      title={web ? t`Remove ${title}?` : t`Remove ${title} from your library?`}
      confirmLabel={
        web
          ? t({ message: "Remove", context: "web game" })
          : t({ message: "Remove", context: "library game" })
      }
      pendingLabel={t`Removing…`}
      isPending={remove.isPending}
      error={remove.error}
      onConfirm={() => remove.mutate()}
      close={close}
    >
      <p>
        {web ? (
          keepsGame ? (
            <Trans>
              Its web version is removed: the offline copy and the progress saved in the game window
              are deleted, with its play sessions. The game's other versions stay in your library.
            </Trans>
          ) : (
            <Trans>
              It leaves your library: the offline copy and the progress saved in the game window are
              deleted, with its play sessions. Add its page again to play it again.
            </Trans>
          )
        ) : uninstalls ? (
          <Trans>
            The files Caskade installed for it are deleted, then it leaves your library with its
            play sessions. Files it didn't install stay where they are. The uninstall shows in
            Activity.
          </Trans>
        ) : (
          <Trans>
            It leaves your library with its play sessions. Its files, if it has any, stay where they
            are.
          </Trans>
        )}
      </p>
    </ConfirmDialog>
  );
}
