import { ContextMenu, type MenuAnchor, MenuItem, MenuSection, MenuSeparator } from "@caskade/ui";
import { useLingui } from "@lingui/react/macro";
import {
  CloudOff,
  Download,
  EyeOff,
  FolderInput,
  FolderOpen,
  Heart,
  HeartOff,
  Info,
  ListPlus,
  MonitorSmartphone,
  Pencil,
  Play,
  Send,
  Square,
  Trash2,
  Wrench,
} from "lucide-react";
import { useGameActions } from "../../api/actions";
import { useCollectionsApi } from "../../api/collections";
import {
  useGame,
  useRevealEntry,
  useSessions,
  useSetFavorite,
  useSetHidden,
  useStopGame,
} from "../../api/hooks";
import { formatSize } from "../../api/presentation";
import { useLibraryApi } from "../../api/provider";
import { useStorageApi, useUnavailableStorageEntries } from "../../api/StorageProvider";
import { streamReleaseName, streamReleaseState, useStreamHosts } from "../../api/streaming";
import { usePeerCopies, useSyncAction, useSyncStatus } from "../../api/sync";
import type { GameEntry } from "../../api/types";
import { useGameActionContributions } from "../../plugins/GameSlots";
import { useMode } from "../../shell/console/mode";
import { usePeerNote } from "../../views/game/OtherDevices";
import { useOpenGame } from "../../views/game/origin";
import { useRemoveOffline } from "../../views/library/web/SaveOffline";
import type { GameMenuDialog, GameMenuTarget } from "./GameMenuHost";

/** The entry Play starts: the game's default when it can launch and is
 * reachable, else the first one that can. */
export function playableEntry(
  entries: readonly GameEntry[],
  unavailable: ReadonlySet<string>,
  preferred?: string | null,
) {
  const playable = (entry: GameEntry) => entry.canLaunch && !unavailable.has(entry.id);
  return (
    entries.find((entry) => entry.id === preferred && playable(entry)) ?? entries.find(playable)
  );
}

/** Caskade's own installs, the ones it can move and uninstall. */
const MANAGED = "caskade-storage";
/** The sources only Caskade adds to, installs, hand-offs and games added by
 * hand (#174): no scan brings their games back, so they can be removed. */
const ADDED = new Set([MANAGED, "caskade-handoff", "manual"]);

/** A game's menu: play or stop it, open its page, mark it a favourite,
 * put it in collections, open its folder, move or uninstall its files, hide
 * it, remove a game Caskade added or a web game, and plugins'
 * `game.actions`. Scoped to one
 * entry (the game page's rows) it keeps the entry's own actions. Items that
 * need the game's details wait for them. */
export function GameMenu({
  target,
  anchor,
  onClose,
  onDialog,
}: {
  target: GameMenuTarget;
  anchor: MenuAnchor;
  onClose: () => void;
  onDialog: (dialog: GameMenuDialog) => void;
}) {
  const { t } = useLingui();
  // Its page goes back to the view the menu was opened on (#195).
  const openGame = useOpenGame();
  const game = useGame(target.gameId).data;
  const sessions = useSessions(target.gameId).data;
  const stop = useStopGame();
  const storage = useStorageApi();
  const unavailable = useUnavailableStorageEntries();
  const setFavorite = useSetFavorite();
  const setHidden = useSetHidden();
  const collections = useCollectionsApi();
  // A file manager opens behind console mode's fullscreen window, where a
  // pad can't reach it.
  const consoleMode = useMode().mode === "console";
  const reveal = useRevealEntry();
  const api = useLibraryApi();
  const removeOffline = useRemoveOffline();
  const plugins = useGameActionContributions();
  // The game's copies on the other devices (#232): each opens its page.
  const peers = usePeerCopies(target.gameId).data ?? [];
  const peerNote = usePeerNote();
  // Devices it can be sent to (#235): reached directly, without a copy,
  // when this computer has one in a folder that can be copied.
  const sendable = (game?.entries ?? []).some(
    (entry) =>
      !/^(peer|store|stream):/.test(entry.source) &&
      entry.state === "installed" &&
      entry.location !== null,
  );
  const paired = useSyncStatus().data?.devices ?? [];
  const targets = sendable
    ? paired.filter(
        (device) =>
          (device.link === "lan" || device.link === "direct") &&
          !peers.some((peer) => peer.deviceId === device.id),
      )
    : [];
  const send = useSyncAction((api, deviceId: string) => api.sendTo(target.gameId, deviceId));
  // Releases' play actions and tools (#178).
  const releases = useGameActions(target.gameId).data ?? [];
  // Releases streamed from other devices (#264), by the device's name.
  const hosts = useStreamHosts().data ?? [];
  const streamName = (entry?: GameEntry) =>
    entry ? streamReleaseName(entry.source, hosts) : undefined;
  const scoped = target.entryId !== undefined;
  const entries: GameEntry[] = (game?.entries ?? []).filter(
    (entry) => !scoped || entry.id === target.entryId,
  );
  const running = sessions?.find(
    (session) => session.endedAt === null && (!scoped || session.entryId === target.entryId),
  );
  const playable = playableEntry(entries, unavailable, game?.defaultEntryId);
  // One per folder: launchers can list one install twice (Lutris imports
  // Steam's games, for one).
  const located = entries.filter(
    (entry, index) =>
      entry.location !== null &&
      entries.findIndex((other) => other.location === entry.location) === index,
  );
  const managed = entries.filter((entry) => entry.source === MANAGED);
  const installed = managed.filter((entry) => entry.state === "installed");
  // A web game plays online, or from its offline copy (a location).
  const web = entries.find((entry) => entry.source === "web");
  // Scans bring back what they find, so a game they found is hidden; they
  // never find web games or what Caskade added, which are removed instead.
  const scanned = (game?.entries ?? []).some((entry) => entry.source !== "web");
  const added = game?.entries.every((entry) => ADDED.has(entry.source)) ?? false;
  // The play actions Play could start, when there is a choice, and the
  // tools of the entries the menu is for.
  const playActions =
    releases
      .find((release) => release.entryId === playable?.id)
      ?.actions.filter((action) => action.kind === "play") ?? [];
  const tools = releases
    .filter((release) => entries.some((entry) => entry.id === release.entryId))
    .flatMap((release) => release.actions.filter((action) => action.kind === "utility"));
  // Several entries: each item names the one it acts on.
  const which = (entry: GameEntry, of: readonly GameEntry[]) =>
    of.length > 1 ? (entry.location ?? entry.source) : undefined;
  const failure =
    reveal?.error?.message ??
    setFavorite?.error?.message ??
    removeOffline?.error?.message ??
    send.error?.message ??
    stop.error?.message;
  return (
    <ContextMenu aria-label={target.title} anchor={anchor} onClose={onClose}>
      <MenuSection>
        {/* Keyed: React Aria refuses an item whose id changes, and Play
            becomes Stop in place when the game starts or its sessions load. */}
        {running ? (
          <MenuItem
            key="stop"
            id="stop"
            icon={Square}
            tone="danger"
            isDisabled={stop.isPending}
            shouldCloseOnSelect={false}
            onAction={() => stop.mutate(running.id, { onSuccess: onClose })}
          >
            {streamName(entries.find((entry) => entry.id === running.entryId))
              ? t({ message: "End stream", context: "game action" })
              : t({ message: "Stop", context: "game action" })}
          </MenuItem>
        ) : (
          <MenuItem
            key="play"
            id="play"
            icon={Play}
            // Enabled while the details load; the launch picks the entry then.
            isDisabled={Boolean(game) && !playable}
            description={
              game && !playable
                ? entries.length
                  ? t`Not installed or unavailable`
                  : t`Nothing to launch`
                : playable && streamReleaseState(playable.source, hosts)
            }
            onAction={() => onDialog({ type: "play", entryId: playable?.id ?? target.entryId })}
          >
            {streamName(playable) ?? t({ message: "Play", context: "game action" })}
          </MenuItem>
        )}
        {!running &&
          entries
            .filter((entry) => entry.id !== playable?.id && streamName(entry) && entry.canLaunch)
            .map((entry) => (
              <MenuItem
                key={`stream:${entry.id}`}
                id={`stream:${entry.id}`}
                icon={Play}
                description={streamReleaseState(entry.source, hosts)}
                onAction={() => onDialog({ type: "play", entryId: entry.id })}
              >
                {streamName(entry)}
              </MenuItem>
            ))}
        {!running &&
          playable &&
          playActions.length > 1 &&
          playActions.map((action) => (
            <MenuItem
              key={`play:${action.id}`}
              id={`play:${action.id}`}
              icon={Play}
              onAction={() => onDialog({ type: "play", entryId: playable.id, action: action.id })}
            >
              {t({
                message: `Play with ${action.name}`,
                comment: "Starts the game with one of its launch actions",
              })}
            </MenuItem>
          ))}
        {!scoped && (
          <MenuItem id="details" icon={Info} onAction={() => openGame(target.gameId)}>
            {t`Open game details`}
          </MenuItem>
        )}
      </MenuSection>
      <MenuSeparator />
      <MenuSection>
        {!scoped && setFavorite && game && (
          <MenuItem
            id="favorite"
            icon={game.favorite ? HeartOff : Heart}
            isDisabled={setFavorite.isPending}
            shouldCloseOnSelect={false}
            onAction={() =>
              setFavorite.mutate(
                { gameId: game.id, favorite: !game.favorite },
                { onSuccess: onClose },
              )
            }
          >
            {game.favorite ? t`Remove from favourites` : t`Add to favourites`}
          </MenuItem>
        )}
        {!scoped && api.edits && (
          <MenuItem id="edit" icon={Pencil} onAction={() => onDialog({ type: "edit" })}>
            {t`Edit…`}
          </MenuItem>
        )}
        {!scoped && collections && (
          <MenuItem
            id="collections"
            icon={ListPlus}
            onAction={() => onDialog({ type: "collections" })}
          >
            {t`Add to collection…`}
          </MenuItem>
        )}
        {reveal &&
          !consoleMode &&
          located.map((entry) => (
            <MenuItem
              key={`reveal:${entry.id}`}
              id={`reveal:${entry.id}`}
              icon={FolderOpen}
              description={which(entry, located)}
              isDisabled={reveal.isPending}
              // A failure shows in the menu.
              shouldCloseOnSelect={false}
              onAction={() => reveal.mutate(entry.id, { onSuccess: onClose })}
            >
              {t`Show in folder`}
            </MenuItem>
          ))}
        {managed.length > 0 && (
          <MenuItem
            id="move"
            icon={FolderInput}
            isDisabled={!storage || Boolean(running)}
            description={running ? t`Quit the game first` : undefined}
            onAction={() => onDialog({ type: "move", entryId: target.entryId })}
          >
            {t`Move installed files…`}
          </MenuItem>
        )}
        {storage &&
          installed.map((entry) => (
            <MenuItem
              key={`uninstall:${entry.id}`}
              id={`uninstall:${entry.id}`}
              icon={Trash2}
              tone="danger"
              isDisabled={Boolean(running)}
              description={running ? t`Quit the game first` : which(entry, installed)}
              onAction={() =>
                onDialog({ type: "uninstall", entryId: entry.id, location: entry.location })
              }
            >
              {t`Uninstall…`}
            </MenuItem>
          ))}
        {web && !web.location && storage && api.saveWebGameOffline && (
          <MenuItem
            id="offline"
            icon={Download}
            isDisabled={Boolean(running)}
            description={running ? t`Quit the game first` : undefined}
            onAction={() => onDialog({ type: "offline" })}
          >
            {t`Play offline…`}
          </MenuItem>
        )}
        {web?.location && removeOffline && (
          <MenuItem
            id="online"
            icon={CloudOff}
            tone="danger"
            isDisabled={Boolean(running) || removeOffline.isPending}
            shouldCloseOnSelect={false}
            description={running ? t`Quit the game first` : t`The game stays, and plays online.`}
            onAction={() => removeOffline.mutate(target.gameId, { onSuccess: onClose })}
          >
            {t`Remove offline copy`}
          </MenuItem>
        )}
        {web && api.removeWebGame && (
          <MenuItem
            id="remove-web"
            icon={Trash2}
            tone="danger"
            isDisabled={Boolean(running)}
            description={running ? t`Quit the game first` : undefined}
            onAction={() => onDialog({ type: "remove-web", keepsGame: scanned })}
          >
            {t({ message: "Remove…", context: "web game" })}
          </MenuItem>
        )}
        {!scoped && api.removeGame && added && (
          <MenuItem
            id="remove"
            icon={Trash2}
            tone="danger"
            isDisabled={Boolean(running)}
            description={running ? t`Quit the game first` : undefined}
            onAction={() =>
              onDialog({
                type: "remove",
                uninstalls: Boolean(game?.entries.some((entry) => entry.source === MANAGED)),
              })
            }
          >
            {t`Remove from library…`}
          </MenuItem>
        )}
        {!scoped && setHidden && game && scanned && (
          <MenuItem
            id="hide"
            icon={EyeOff}
            isDisabled={setHidden.isPending}
            shouldCloseOnSelect={false}
            description={
              setHidden.error?.message ??
              t`Scans won't bring it back. Show it again in Settings → Library Discovery.`
            }
            onAction={() =>
              setHidden.mutate({ gameId: target.gameId, hidden: true }, { onSuccess: onClose })
            }
          >
            {t`Hide from library`}
          </MenuItem>
        )}
        {failure && (
          <MenuItem id="failure" isDisabled description={failure}>
            {t({
              message: "That didn't work",
              comment: "A menu action failed; the reason is below",
            })}
          </MenuItem>
        )}
      </MenuSection>
      {!scoped && (peers.length > 0 || targets.length > 0) && (
        <>
          <MenuSeparator />
          <MenuSection title={t({ message: "Your devices", context: "game menu section" })}>
            {targets.map((device) => {
              const name = device.name;
              return (
                <MenuItem
                  key={`send:${device.id}`}
                  id={`send:${device.id}`}
                  icon={Send}
                  description={t`It asks there before copying`}
                  onAction={() => send.mutate(device.id)}
                >
                  {t({
                    message: `Copy to ${name}`,
                    comment: "Sends the game to another of the user's devices",
                  })}
                </MenuItem>
              );
            })}
            {peers.map((peer) => {
              const device = peer.deviceName;
              return (
                <MenuItem
                  key={`device:${peer.deviceId}`}
                  id={`device:${peer.deviceId}`}
                  icon={MonitorSmartphone}
                  description={
                    [peer.sizeBytes === null ? null : formatSize(peer.sizeBytes), peerNote(peer)]
                      .filter(Boolean)
                      .join(" · ") || undefined
                  }
                  onAction={() => openGame(target.gameId)}
                >
                  {t({
                    message: `On ${device}`,
                    comment: "The game is installed on another of the user's devices",
                  })}
                </MenuItem>
              );
            })}
          </MenuSection>
        </>
      )}
      {tools.length > 0 && (
        <>
          <MenuSeparator />
          <MenuSection title={t({ message: "Tools", context: "game menu section" })}>
            {tools.map((tool) => (
              <MenuItem
                key={`tool:${tool.id}`}
                id={`tool:${tool.id}`}
                icon={Wrench}
                description={t`No playtime is counted`}
                onAction={() =>
                  onDialog({
                    type: "tool",
                    entryId: tool.entryId,
                    actionId: tool.id,
                    name: tool.name,
                  })
                }
              >
                {tool.name}
              </MenuItem>
            ))}
          </MenuSection>
        </>
      )}
      {!scoped && plugins.actions.length > 0 && (
        <>
          <MenuSeparator />
          <MenuSection title={t({ message: "Plugins", context: "game menu section" })}>
            {plugins.actions.map((action) => (
              <MenuItem
                key={`${action.plugin.id}/${action.contribution.id}`}
                id={`plugin:${action.plugin.id}/${action.contribution.id}`}
                data-plugin={action.plugin.id}
                onAction={() => plugins.run(action, target.gameId)}
              >
                {action.contribution.title}
              </MenuItem>
            ))}
          </MenuSection>
        </>
      )}
    </ContextMenu>
  );
}
