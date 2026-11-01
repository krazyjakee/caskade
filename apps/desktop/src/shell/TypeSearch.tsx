/** Search from a real keyboard, RetroArch style: typing anywhere outside a
 * text field, or `/`, opens a centred overlay that shows only the text and
 * how many games match. In the Library the games filter as it changes
 * (within its filter and platform); anywhere else Enter opens the Library
 * on the results. A pad types on the on-screen keyboard instead, and the
 * topbar's search box stays for the mouse and Ctrl K. */
import { plural } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { useNavigate, useRouterState, useSearch } from "@tanstack/react-router";
import { Search } from "lucide-react";
import {
  createContext,
  type KeyboardEvent,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
} from "react";
import { useMatchCount } from "../api/hooks";
import type { LibraryFilter, LibraryLayout, LibrarySort } from "../api/types";
import { useInputAction } from "../input";
import { isTextEntry } from "../input/actions";
import { focus } from "../input/spatial";

/** Whether `event` starts a search: a printable character typed outside a
 * text field, a menu, a popover or a dialog. Keys the console-mode
 * keyboard map turns into actions never get here. */
export function startsTyping(event: globalThis.KeyboardEvent): boolean {
  if (event.defaultPrevented || event.isComposing) return false;
  if (event.ctrlKey || event.metaKey || event.altKey) return false;
  if ([...event.key].length !== 1 || event.key.trim() === "") return false;
  const target = event.target instanceof Element ? event.target : null;
  if (isTextEntry(target)) return false;
  return !target?.closest(
    "[role='dialog'], [role='alertdialog'], [role='menu'], [role='listbox'], [data-placement]",
  );
}

interface TypeSearchState {
  /** Opens the overlay with `text` typed. */
  open(text: string): void;
}
const Context = createContext<TypeSearchState | null>(null);
export const useTypeSearch = () => useContext(Context);

interface LibrarySearch {
  filter?: LibraryFilter;
  q?: string;
  sort?: LibrarySort;
  layout?: LibraryLayout;
  platform?: string;
}

/** Focuses the Library's first game once it is on screen. */
function focusFirstResult() {
  let frames = 0;
  const attempt = () => {
    const first = document.querySelector<HTMLElement>(
      '[data-view="library"] :is([data-cs="game-card"], [data-cs="game-row"]) a[href]',
    );
    if (first) focus(first, true);
    else if (++frames < 60) requestAnimationFrame(attempt);
  };
  requestAnimationFrame(attempt);
}

export function TypeSearchProvider({ children }: { children: ReactNode }) {
  const navigate = useNavigate();
  const pathname = useRouterState({ select: (state) => state.location.pathname });
  const search = useSearch({ strict: false }) as LibrarySearch;
  const inLibrary = pathname === "/library";
  const [text, setText] = useState<string | null>(null);
  // The search and focus from before it opened, for Escape.
  const before = useRef<{ query?: string; focus: Element | null }>({ focus: null });
  const latest = useRef({ inLibrary, search });
  latest.current = { inLibrary, search };

  /** In the Library, the text is its search (`q`), in place. */
  const filter = useCallback(
    (q: string | undefined) => {
      const { filter, sort, layout, platform } = latest.current.search;
      void navigate({
        to: "/library",
        search: { filter, q: q?.trim() || undefined, sort, layout, platform },
        replace: true,
      });
    },
    [navigate],
  );
  const type = useCallback(
    (next: string) => {
      setText(next);
      const { inLibrary, search } = latest.current;
      if (inLibrary && (next.trim() || undefined) !== search.q) filter(next);
    },
    [filter],
  );
  const open = useCallback(
    (initial: string) => {
      const { inLibrary, search } = latest.current;
      before.current = { query: inLibrary ? search.q : undefined, focus: document.activeElement };
      type(initial);
    },
    [type],
  );
  const isOpen = text !== null;
  useEffect(() => {
    if (isOpen) return;
    const onKey = (event: globalThis.KeyboardEvent) => {
      if (!startsTyping(event)) return;
      event.preventDefault();
      // `/` is the search key; anything else is the first character.
      open(event.key === "/" ? "" : event.key);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [isOpen, open]);

  const refocus = () => {
    const { focus: was } = before.current;
    if (was instanceof HTMLElement && was.isConnected) was.focus();
  };
  const close = () => setText(null);
  const query = latest.current.inLibrary
    ? {
        filter: search.filter ?? "all",
        sort: search.sort ?? "title",
        platform: search.platform,
        query: text ?? "",
      }
    : { filter: "all" as const, sort: "title" as const, query: text ?? "" };
  const count = useMatchCount(query, { enabled: Boolean(text?.trim()) });
  return (
    <Context value={{ open }}>
      {children}
      {text !== null && (
        <TypeSearch
          value={text}
          count={text.trim() ? count.data : undefined}
          onChange={type}
          onBrowse={() => {
            close();
            const q = text.trim();
            if (!q) return refocus();
            if (!inLibrary) void navigate({ to: "/library", search: { filter: "all", q } });
            focusFirstResult();
          }}
          onCancel={() => {
            close();
            if (inLibrary) filter(before.current.query);
            refocus();
          }}
          onLeave={(focusBack) => {
            close();
            if (focusBack) refocus();
          }}
        />
      )}
    </Context>
  );
}

/** The overlay: the text being typed and the count. Enter or Down goes to
 * the results, Escape puts the earlier search back, and Backspace on an
 * empty line closes it. */
function TypeSearch({
  value,
  count,
  onChange,
  onBrowse,
  onCancel,
  onLeave,
}: {
  value: string;
  /** Games matching so far; undefined without text or before the first answer. */
  count?: number;
  onChange: (value: string) => void;
  onBrowse: () => void;
  onCancel: () => void;
  /** Closes it, keeping the search: focus left it, or it was emptied. */
  onLeave: (refocus: boolean) => void;
}) {
  const { t } = useLingui();
  const ref = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    input.current?.focus();
  }, []);
  // With the console keyboard map on, Escape and Down arrive as actions.
  useInputAction("back", onCancel, {
    owner: ref,
    label: t({ message: "Cancel", context: "pad action" }),
  });
  useInputAction("nav.down", onBrowse, {
    owner: ref,
    label: t({ message: "Results", comment: "Hint bar: go down to the search results" }),
  });
  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Enter" || event.key === "ArrowDown") onBrowse();
    else if (event.key === "Escape") onCancel();
    else if (event.key === "Backspace" && value === "") onLeave(true);
    else return;
    event.preventDefault();
  };
  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-surface-base/40 p-6">
      <div
        ref={ref}
        role="dialog"
        aria-label={t({ message: "Search", context: "noun" })}
        data-cs="type-search"
        className="flex w-full max-w-3xl animate-enter flex-col gap-3 rounded-panel border border-accent-strong bg-surface-overlay px-8 py-6 shadow-xl shadow-black/40"
      >
        <p
          data-part="label"
          className="flex items-center gap-2 font-mono text-xs tracking-widest text-accent-alt uppercase"
        >
          <Search className="size-4" aria-hidden />
          <Trans context="noun">Search</Trans>
        </p>
        <div className="flex items-baseline gap-4">
          <input
            ref={input}
            data-part="input"
            aria-label={t`Search games`}
            value={value}
            onChange={(event) => onChange(event.target.value)}
            onKeyDown={onKeyDown}
            onBlur={() => onLeave(false)}
            spellCheck={false}
            autoComplete="off"
            className="min-w-0 flex-1 bg-transparent font-display text-4xl text-fg caret-accent-alt outline-none"
          />
          <span
            data-part="count"
            role="status"
            className="shrink-0 font-mono text-sm text-fg-muted"
          >
            {count === undefined
              ? ""
              : t({ message: plural({ total: count }, { one: "# game", other: "# games" }) })}
          </span>
        </div>
        <p data-part="hints" className="font-mono text-xs tracking-wide text-fg-subtle uppercase">
          <Trans comment="Keyboard keys in the search overlay">
            Enter for results · Esc to cancel · Backspace to delete
          </Trans>
        </p>
      </div>
    </div>
  );
}
