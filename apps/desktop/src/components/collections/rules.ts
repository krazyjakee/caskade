import { i18n, type MessageDescriptor } from "@lingui/core";
import { msg, t } from "@lingui/core/macro";
import type { LibraryFilter, LibraryQuery, LibrarySort } from "../../api/types";
import { presentPlatform } from "../../views/library/platforms/catalog";

/** The library filters' names, translated where they're shown (`t(message)`). */
export const filterMessages: Record<LibraryFilter, MessageDescriptor> = {
  all: msg({ message: "All games", context: "library filter" }),
  installed: msg({ message: "Installed", context: "library filter" }),
  owned: msg({
    message: "Ready to Install",
    context: "library filter",
    comment: "Games a connected store account owns that aren't installed",
  }),
  recent: msg({
    message: "Played",
    context: "library filter",
    comment: "Games that have been played",
  }),
  emulated: msg({ message: "Emulated", context: "library filter" }),
  favorites: msg({ message: "Favorites", context: "library filter" }),
  web: msg({
    message: "Web",
    context: "library filter",
    comment: "Games played in the browser",
  }),
};

/** The library sorts' names, translated where they're shown (`t(message)`). */
export const sortMessages: Record<LibrarySort, MessageDescriptor> = {
  title: msg({ message: "Title", context: "library sort" }),
  recent: msg({ message: "Recently played", context: "library sort" }),
  playtime: msg({ message: "Playtime", context: "library sort" }),
  added: msg({ message: "Recently added", context: "library sort" }),
  popular: msg({ message: "Popular", context: "library sort" }),
  released: msg({ message: "Release date", context: "library sort" }),
};

/** `messages` translated whenever a key is read, in the active language. */
function translated<K extends string>(messages: Record<K, MessageDescriptor>): Record<K, string> {
  const labels = {} as Record<K, string>;
  for (const key of Object.keys(messages) as K[])
    Object.defineProperty(labels, key, { enumerable: true, get: () => i18n._(messages[key]) });
  return labels;
}

/** `filterMessages` in the active language, read as plain strings. */
const filterLabels = translated(filterMessages);

interface Rule {
  field: string;
  op: string;
  value: string;
}

/** A smart collection's query as rules its games all match, e.g.
 * "Games are Installed", "Platform is SNES", in the active language. */
export function describeRules(query: LibraryQuery): Rule[] {
  const games = (): Rule => ({
    field: t({
      message: "Games",
      context: "smart collection rule",
      comment: "The first word of a rule such as “Games are Installed”",
    }),
    op: t({ message: "are", context: "smart collection rule" }),
    value: filterLabels[query.filter],
  });
  const rules: Rule[] = [];
  if (query.filter !== "all") rules.push(games());
  if (query.platform)
    rules.push({
      field: t({ message: "Platform", context: "smart collection rule" }),
      op: t({ message: "is", context: "smart collection rule" }),
      value: presentPlatform(query.platform).title,
    });
  const text = query.query;
  if (text)
    rules.push({
      field: t({ message: "Title", context: "smart collection rule" }),
      op: t({
        message: "has",
        context: "smart collection rule",
        comment: "As in “Title has ‘mario’”",
      }),
      value: t({
        message: `“${text}”`,
        comment: "The title text a smart collection looks for, in quotes",
      }),
    });
  return rules.length ? rules : [{ ...games(), value: filterLabels.all }];
}
