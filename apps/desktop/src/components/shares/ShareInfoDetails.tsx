/** What a share link shares, as its sharer says: who, what, how big, for
 * which system, until when, and whether it comes directly; for something
 * smaller than a game (#248), where it goes here; for a collection, which of
 * its games this device has. */
import type { MessageDescriptor } from "@lingui/core";
import { msg } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { formatDateTime, formatSize } from "../../api/presentation";
import type { GameHere, ShareInfo } from "../../api/shares";

const here: Record<GameHere, MessageDescriptor> = {
  installed: msg({
    message: "Installed",
    comment: "A game of a shared collection, on this device",
  }),
  owned: msg({ message: "Owned", comment: "A game of a shared collection a store account owns" }),
  inLibrary: msg({ message: "In your library", comment: "A game of a shared collection" }),
  missing: msg({ message: "Not in your library", comment: "A game of a shared collection" }),
};

export function ShareInfoDetails({ info }: { info: ShareInfo }) {
  const { t } = useLingui();
  const sharer = info.sharer;
  const title = info.title;
  const contents = info.contents;
  const until = info.expiresAtMs === null ? null : formatDateTime(info.expiresAtMs);
  const rows = [
    { label: t({ message: "From", comment: "Who shared something" }), value: sharer },
    info.size !== null &&
      contents.kind !== "collection" && {
        label: t`Size`,
        value: formatSize(info.size),
      },
    info.windows && {
      label: t({ message: "Build", comment: "Which system a shared game was built for" }),
      value: t`Windows (runs through Proton on Linux)`,
    },
    contents.kind === "web" && {
      label: t({ message: "Address", comment: "A shared web game's address" }),
      value: contents.url,
    },
    contents.kind === "rom" && {
      label: t({ message: "Platform", comment: "A shared ROM's platform" }),
      value: contents.platform,
    },
    {
      label: t({ message: "Link", comment: "How long a share link works" }),
      value: until ? t`Works until ${until}` : t`Works until it's stopped`,
    },
  ].filter((row): row is { label: string; value: string } => Boolean(row));
  return (
    <div data-cs="share-info" data-status={info.reach} className="space-y-3">
      <p className="text-sm">
        {contents.kind === "save" ? (
          <Trans>
            {sharer} shares a save of {contents.game} with you.
          </Trans>
        ) : contents.kind === "collection" ? (
          <Trans>
            {sharer} shares the collection {contents.name} with you.
          </Trans>
        ) : (
          <Trans>
            {sharer} shares {title} with you.
          </Trans>
        )}
      </p>
      <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1.5 rounded-control border border-border p-3 text-xs">
        {rows.map((row) => (
          <div key={row.label} className="contents">
            <dt className="text-fg-subtle">{row.label}</dt>
            <dd className="break-all">{row.value}</dd>
          </div>
        ))}
      </dl>
      <Where info={info} />
      {info.reach === "relayOnly" && contents.kind !== "collection" && (
        <p role="status" className="text-xs text-warning">
          <Trans>
            Only a relay reaches {sharer}'s computer right now. The game waits for a direct
            connection: game files don't go through relays.
          </Trans>
        </p>
      )}
    </div>
  );
}

/** Where what's shared goes here. */
function Where({ info }: { info: ShareInfo }) {
  const { t } = useLingui();
  const contents = info.contents;
  switch (contents.kind) {
    case "game":
      return null;
    case "save": {
      const game = contents.game;
      return (
        <p data-part="where" className="text-xs text-fg-muted">
          {contents.here ? (
            <Trans>
              It arrives as a new version of your saves of {game}. Your own saves stay: the next
              Play asks which to use.
            </Trans>
          ) : (
            <Trans>
              {game} isn't in your library: Caskade adds it, with this save as its first version.
            </Trans>
          )}
        </p>
      );
    }
    case "web":
      return (
        <p data-part="where" className="text-xs text-fg-muted">
          {contents.here ? (
            <Trans>It becomes the offline copy of the web game you have at this address.</Trans>
          ) : (
            <Trans>Caskade adds it to your web games, ready to play offline.</Trans>
          )}
        </p>
      );
    case "rom": {
      const emulator = contents.emulator;
      return (
        <p data-part="where" className="text-xs text-fg-muted">
          {contents.ready ? (
            <Trans>This device has an emulator for it.</Trans>
          ) : emulator ? (
            <Trans>Caskade sets up {emulator} to play it.</Trans>
          ) : (
            <Trans>Caskade sets up an emulator to play it.</Trans>
          )}
        </p>
      );
    }
    case "collection":
      return (
        <ul data-part="games" className="max-h-48 space-y-1 overflow-auto text-xs">
          {contents.games.map((game) => (
            <li key={`${game.title}:${game.igdbId ?? ""}`} className="flex gap-3">
              <span className="min-w-0 flex-1 truncate">{game.title}</span>
              <span data-status={game.here} className="text-fg-subtle">
                {t(here[game.here])}
              </span>
            </li>
          ))}
        </ul>
      );
  }
}
