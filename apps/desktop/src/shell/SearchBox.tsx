import { GameArt, StatusBadge } from "@caskade/ui";
import { plural } from "@lingui/core/macro";
import { useLingui } from "@lingui/react/macro";
import { useNavigate, useSearch } from "@tanstack/react-router";
import { CornerDownLeft, Search } from "lucide-react";
import { type KeyboardEvent, useEffect, useRef, useState } from "react";
import { ComboBox, Group, Input, ListBox, ListBoxItem, Popover } from "react-aria-components";
import { useSearchGames } from "../api/hooks";
import { findFolded } from "../api/presentation";
import { useGameMenus, useSelectGame } from "../components/gameMenu/GameMenuHost";
import { AgeLockLabel } from "../views/game/AgeGate";
import { glyph } from "./console/glyphs";
import { useHintFamily } from "./console/HintBar";
import { useMode } from "./console/mode";

/** How long typing pauses before the suggestions are fetched. */
const SUGGEST_DELAY_MS = 150;

/** Wraps the first occurrence of `query` in a <mark>, case and accents
 * aside as the search matches ("pokemon" marks "Pokémon"). */
function Highlight({ text, query }: { text: string; query: string }) {
  const found = findFolded(text, query);
  if (!found) return text;
  const [at, end] = found;
  return (
    <>
      {text.slice(0, at)}
      <mark className="bg-transparent text-accent-alt">{text.slice(at, end)}</mark>
      {text.slice(end)}
    </>
  );
}

/**
 * Topbar search: suggests games as you type. Enter (or "See all results")
 * opens the Library filtered by the typed text. Picking a suggestion
 * chooses the game as a card click does: its summary panel in desktop mode,
 * its page (which goes back to this view) in console mode.
 */
export function SearchBox() {
  const { t } = useLingui();
  const navigate = useNavigate();
  // A right click on a suggestion opens its game's menu.
  const menus = useGameMenus();
  const select = useSelectGame();
  const inputRef = useRef<HTMLInputElement>(null);
  // Console mode names the pad's search button (Y), or `/` on the keyboard.
  const consoleMode = useMode().mode === "console";
  const family = useHintFamily();
  const shortcut = consoleMode
    ? glyph("search", family)
    : t({
        message: "CTRL K",
        context: "keyboard key",
        comment: "The keyboard shortcut that focuses the search",
      });
  // Set when Enter triggered an item action, so the Enter handler doesn't search too.
  const handledByItem = useRef(false);

  // Mirror the Library's ?q= into the field, so it shows what the results are for.
  const { q } = useSearch({ strict: false });
  const [input, setInput] = useState(q ?? "");
  const [syncedQ, setSyncedQ] = useState(q);
  if (q !== syncedQ) {
    setSyncedQ(q);
    setInput(q ?? "");
  }

  // Suggestions follow the first keystroke, then the typing once it
  // pauses, not every keystroke.
  const [typed, setTyped] = useState(input);
  const typing = useRef(false);
  useEffect(() => {
    if (!typing.current) setTyped(input);
    typing.current = true;
    const wait = setTimeout(() => {
      typing.current = false;
      setTyped(input);
    }, SUGGEST_DELAY_MS);
    return () => clearTimeout(wait);
  }, [input]);
  const result = useSearchGames(input.trim() ? typed : "");
  const suggestions = result.data?.items ?? [];
  // An empty collection closes the popover.
  const hasQuery = input.trim() !== "";
  const query = input.trim();
  const total = result.data?.total ?? 0;

  const search = (query: string) => {
    handledByItem.current = true;
    inputRef.current?.blur();
    const trimmed = query.trim();
    if (!trimmed) return;
    setInput(trimmed);
    navigate({ to: "/library", search: { filter: "all", q: trimmed } });
  };

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key !== "Enter") return;
    if (handledByItem.current) return;
    search(input);
  };

  // Ctrl/Cmd+K focuses the search from anywhere.
  useEffect(() => {
    const onKey = (e: globalThis.KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        inputRef.current?.focus();
        inputRef.current?.select();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <ComboBox
      data-cs="search"
      // The padding around the input belongs to the search, not the title bar.
      data-window-drag="false"
      aria-label={t`Search games`}
      inputValue={input}
      onInputChange={setInput}
      allowsCustomValue
      menuTrigger="input"
      // SQLite FTS5 has already filtered and bounded these results.
      defaultFilter={() => true}
      className="w-full max-w-xl"
    >
      <Group
        data-part="control"
        className="flex h-11 items-center gap-3 rounded-control border border-border bg-surface-raised px-4 text-sm transition-[border-color,box-shadow] duration-(--cs-motion-duration-base) data-focus-within:border-accent-alt/50 data-focus-within:shadow-[0_0_20px_-8px] data-focus-within:shadow-accent-alt"
      >
        <Search data-part="icon" className="size-4 shrink-0 text-fg-subtle" />
        <Input
          ref={inputRef}
          data-part="input"
          placeholder={t`Search any game… (classic, modern, or something obscure)`}
          onKeyDownCapture={() => {
            handledByItem.current = false;
          }}
          onKeyDown={onKeyDown}
          className="w-full bg-transparent placeholder:text-fg-subtle focus:outline-none"
        />
        {shortcut && (
          <kbd
            data-part="shortcut"
            className="whitespace-nowrap rounded border border-border-strong px-1.5 py-0.5 font-mono text-[10px] text-fg-subtle"
          >
            {shortcut}
          </kbd>
        )}
      </Group>

      <Popover
        data-cs="search-popover"
        offset={6}
        className="w-(--trigger-width) rounded-control border border-border-strong bg-surface-overlay p-1 shadow-xl shadow-black/40 data-entering:animate-enter"
      >
        <ListBox className="max-h-[28rem] overflow-y-auto outline-none">
          {suggestions.map((game) => (
            <ListBoxItem
              key={game.id}
              id={`game:${game.id}`}
              textValue={game.title}
              onAction={() => {
                handledByItem.current = true;
                inputRef.current?.blur();
                select({ gameId: game.id, title: game.title });
              }}
              onContextMenu={menus({ gameId: game.id, title: game.title }).onContextMenu}
              data-part="suggestion"
              className="flex cursor-default items-center gap-3 rounded-[6px] px-2 py-1.5 outline-none data-focused:bg-surface-hover"
            >
              <GameArt
                game={game}
                image={game.coverList}
                className="size-9 shrink-0 rounded-[4px]"
              />
              <span className="flex min-w-0 flex-1 flex-col">
                <span className="truncate text-sm">
                  <Highlight text={game.title} query={input} />
                </span>
                <span className="truncate text-xs text-fg-subtle">
                  <Highlight text={game.genre} query={input} /> ·{" "}
                  <span className="font-mono">
                    <Highlight text={game.platform} query={input} />
                  </span>
                </span>
              </span>
              {game.adminPinNeeded && <AgeLockLabel />}
              <StatusBadge status={game.status} />
            </ListBoxItem>
          ))}
          {hasQuery && (
            <ListBoxItem
              id="action:search"
              textValue={t`See all results for ${input}`}
              onAction={() => search(input)}
              data-part="see-all"
              className="mt-1 flex cursor-default items-center gap-2 rounded-[6px] px-2 py-2 text-sm text-fg-muted outline-none data-focused:bg-surface-hover data-focused:text-fg"
            >
              <Search className="size-4 text-fg-subtle" />
              <span className="flex-1 truncate">
                {total > 0
                  ? t({
                      message: plural(total, {
                        one: `See # result for “${query}”`,
                        other: `See all # results for “${query}”`,
                      }),
                    })
                  : t`No matches. Search the library for “${query}”`}
              </span>
              <CornerDownLeft className="size-3.5 text-fg-subtle" />
            </ListBoxItem>
          )}
        </ListBox>
      </Popover>
    </ComboBox>
  );
}
