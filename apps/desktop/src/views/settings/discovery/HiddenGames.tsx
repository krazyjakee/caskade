/** The games hidden from a card's actions. Scans that find them again leave
 * them hidden; here they can be shown again. */
import { Button, SettingsSection } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { Eye } from "lucide-react";
import { useHiddenGames, useSetHidden } from "../../../api/hooks";
import { useActionMenus } from "../../../components/ActionMenu";

export function HiddenGames() {
  const { t } = useLingui();
  const games = useHiddenGames();
  const setHidden = useSetHidden();
  const menus = useActionMenus();
  if (!setHidden) return null;
  return (
    <SettingsSection
      title={t`Hidden games`}
      description={t`Hidden from the library. Scans that find them again leave them hidden.`}
    >
      {games.isError ? (
        <p role="alert" className="mt-3 text-sm text-danger">
          <Trans>Couldn't load the hidden games: {games.error.message}</Trans>
        </p>
      ) : games.data?.length ? (
        <ul aria-label={t`Hidden games`} className="mt-3 space-y-2">
          {games.data.map((game) => (
            <li
              key={game.id}
              {...menus.target(game.title, [
                {
                  id: "show",
                  label: t`Show again`,
                  icon: Eye,
                  isDisabled: setHidden.isPending,
                  onAction: () => setHidden.mutate({ gameId: game.id, hidden: false }),
                },
              ])}
              className="flex items-center justify-between gap-3"
            >
              <span className="min-w-0 truncate text-sm">
                {game.title}
                {game.platform && (
                  <span className="ml-2 text-xs text-fg-muted">{game.platform}</span>
                )}
              </span>
              <Button
                size="sm"
                icon={Eye}
                aria-label={t`Show ${{ title: game.title }} again`}
                isDisabled={setHidden.isPending}
                onPress={() => setHidden.mutate({ gameId: game.id, hidden: false })}
              >
                <Trans>Show again</Trans>
              </Button>
            </li>
          ))}
        </ul>
      ) : (
        <p className="mt-3 text-sm text-fg-muted">
          {games.isPending
            ? t`Loading hidden games…`
            : t`No hidden games. Hide one from its card's actions.`}
        </p>
      )}
      {menus.menu}
      {setHidden.error && (
        <p role="alert" className="mt-2 text-sm text-danger">
          {setHidden.error.message}
        </p>
      )}
    </SettingsSection>
  );
}
