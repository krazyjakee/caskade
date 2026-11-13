/** Settings search (#46): a field above the section list that finds sections
 * by their names, the preferences inside them and other words for them
 * (`searchIndex.ts`). A result opens its section, and its sub-section when it
 * has one, then highlights the preference it named and moves focus to it.
 * A pad types in the field on the on-screen keyboard like any other. */
import { TextField } from "@caskade/ui";
import type { MessageDescriptor } from "@lingui/core";
import { Trans, useLingui } from "@lingui/react/macro";
import { Search } from "lucide-react";
import { type RefObject, useEffect } from "react";
import { Button as AriaButton } from "react-aria-components";
import { focus, reducedMotion } from "../../input/spatial";
import type { SettingsTab } from "../SettingsView";
import { fold, matches, sectionWords, settingsSearchIndex } from "./searchIndex";

interface SettingsSearchResult {
  key: string;
  tab: SettingsTab;
  section?: string;
  /** The preference's label as its panel shows it; none for a whole section. */
  label?: string;
}

/** What the last chosen result asked to highlight, until its section shows it. */
export interface SettingsFind {
  tab: SettingsTab;
  label: string;
  /** The section open when it was chosen. */
  from: SettingsTab;
  /** When it was chosen: one left waiting behind a question goes stale. */
  at: number;
}

interface Section {
  id: SettingsTab;
  label: MessageDescriptor;
}

/** The results for `query` among `sections` (the ones this account sees):
 * sections first, then preferences, each those whose name matches before
 * those found by another word. */
function searchSettings(
  query: string,
  sections: readonly Section[],
  translate: (descriptor: MessageDescriptor) => string,
): (SettingsSearchResult & { title: string; within?: string })[] {
  const names = new Map(sections.map((section) => [section.id, translate(section.label)]));
  const found = sections.flatMap((section) => {
    const title = translate(section.label);
    if (!matches(query, [title, translate(sectionWords[section.id])])) return [];
    const result = { key: section.id, tab: section.id, title };
    return [{ result, named: matches(query, [title]) }];
  });
  const preferences = settingsSearchIndex.flatMap((entry) => {
    const within = names.get(entry.tab);
    if (within === undefined) return [];
    const label = translate(entry.label);
    if (!matches(query, [label, entry.words ? translate(entry.words) : ""])) return [];
    const result = {
      key: `${entry.tab}\n${entry.section ?? ""}\n${label}`,
      tab: entry.tab,
      section: entry.section,
      label,
      title: label,
      within,
    };
    return [{ result, named: matches(query, [label]) }];
  });
  const byName = (a: { named: boolean }, b: { named: boolean }) =>
    Number(b.named) - Number(a.named);
  return [...found.sort(byName), ...preferences.sort(byName)]
    .map(({ result }) => result)
    .slice(0, 30);
}

export function SettingsSearch({
  query,
  onQueryChange,
  sections,
  onOpen,
}: {
  query: string;
  onQueryChange: (query: string) => void;
  sections: readonly Section[];
  onOpen: (result: SettingsSearchResult) => void;
}) {
  const { t } = useLingui();
  const results = query.trim() ? searchSettings(query, sections, t) : [];
  return (
    <div className="flex min-h-0 flex-col gap-2 px-2 pt-2">
      <TextField
        type="search"
        aria-label={t`Search settings`}
        placeholder={t`Search settings`}
        icon={Search}
        value={query}
        onChange={onQueryChange}
        onKeyDown={(event) => {
          if (event.key === "Enter" && results[0]) onOpen(results[0]);
          else if (event.key === "Escape" && query) onQueryChange("");
          else event.continuePropagation();
        }}
      />
      {query.trim() &&
        (results.length === 0 ? (
          <p role="status" className="px-1 text-sm text-fg-muted">
            <Trans>No settings match “{query}”.</Trans>
          </p>
        ) : (
          <ul
            aria-label={t`Settings search results`}
            className="-mx-1 flex min-h-0 flex-col gap-1 overflow-y-auto overscroll-contain p-1"
          >
            {results.map((result) => (
              <li key={result.key}>
                <AriaButton
                  aria-label={
                    result.within
                      ? t({
                          message: `${{ title: result.title }} in ${{ section: result.within }}`,
                          comment: "A settings search result: a preference and its section",
                        })
                      : result.title
                  }
                  onPress={() => onOpen(result)}
                  className="flex w-full cursor-pointer flex-col items-start rounded-control border border-transparent px-3 py-2 text-left text-sm text-fg-muted transition-colors duration-(--cs-motion-duration-fast) outline-none data-focus-visible:focus-ring data-hovered:bg-surface-raised data-hovered:text-fg"
                >
                  <span className="min-w-0 break-words text-fg">{result.title}</span>
                  {result.within && (
                    <span aria-hidden className="text-xs text-fg-muted">
                      {result.within}
                    </span>
                  )}
                </AriaButton>
              </li>
            ))}
          </ul>
        ))}
    </div>
  );
}

/** Where a preference's label is shown: a row's label, a field's label or a
 * group's heading. */
const LABELS = '[data-part="label"], [data-part="title"], h2, h3, legend';
/** What gets highlighted around a label. */
const AROUND = '[data-cs="setting-row"], [data-cs="text-field"], [data-cs="settings-section"]';
/** A control to move focus to, in a row or field. */
const CONTROL =
  'input:not([type="hidden"]), textarea, select, button, [role="switch"], [role="slider"], a[href], [tabindex="0"]';

function locate(panel: HTMLElement, label: string): HTMLElement | null {
  const wanted = fold(label).trim();
  for (const element of panel.querySelectorAll<HTMLElement>(LABELS)) {
    if (fold(element.textContent ?? "").trim() !== wanted) continue;
    return element.closest<HTMLElement>(AROUND) ?? element;
  }
  return null;
}

/** How long a found preference stays highlighted. */
const HIGHLIGHT_MS = 4000;
/** How long a section's panel may take to show the preference. */
const WAIT_MS = 5000;
/** A chosen result older than this no longer highlights. */
const STALE_MS = 60_000;

/** Highlights the preference `find` names once `tab`'s panel shows it, and
 * moves focus to its control. `done` clears the request. */
export function useFindInPanel(
  panel: RefObject<HTMLElement | null>,
  tab: SettingsTab,
  find: SettingsFind | null,
  done: () => void,
) {
  useEffect(() => {
    if (!find) return;
    if (find.tab !== tab) {
      // Gone to a third section, or chosen long ago and left waiting behind
      // the unsaved-changes question: forget it.
      if (tab !== find.from || Date.now() - find.at > STALE_MS) done();
      return;
    }
    const root = panel.current;
    if (!root) return;
    // The highlight outlives this effect, which ends as soon as `done` runs.
    const show = (element: HTMLElement) => {
      element.setAttribute("data-found", "");
      setTimeout(() => element.removeAttribute("data-found"), HIGHLIGHT_MS);
      const control =
        element.matches('[data-cs="setting-row"], [data-cs="text-field"]') &&
        element.querySelector<HTMLElement>(CONTROL);
      if (control) focus(control, reducedMotion());
      element.scrollIntoView?.({
        block: "center",
        behavior: reducedMotion() ? "auto" : "smooth",
      });
      done();
    };
    const now = locate(root, find.label);
    if (now) {
      show(now);
      return;
    }
    // The panel may still be loading its data.
    const observer = new MutationObserver(() => {
      const found = locate(root, find.label);
      if (!found) return;
      observer.disconnect();
      show(found);
    });
    observer.observe(root, { subtree: true, childList: true, characterData: true });
    const giveUp = setTimeout(() => {
      observer.disconnect();
      done();
    }, WAIT_MS);
    return () => {
      observer.disconnect();
      clearTimeout(giveUp);
    };
  }, [panel, tab, find, done]);
}
