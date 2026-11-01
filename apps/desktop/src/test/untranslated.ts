/** Finds English left in a view rendered in the pseudo-locale (#217), as
 * `pseudo.test.tsx` does for the shells: pseudo text keeps no ASCII letter
 * but v and x, so two plain letters in a row missed the catalog. */

const english = /[a-uwyzA-UWYZ]{2,}/;

const attributes = ["aria-label", "title", "placeholder", "alt", "aria-description"];

export interface UntranslatedOptions {
  /** Text that is data, not interface: names, paths, brands. */
  data?: (RegExp | string)[];
  /** Subtrees left out: published documents, data lists. */
  skip?: string;
}

/** What in `roots` shows English. A message's text, and the values filled
 * into it, sit inside its brackets. */
export function untranslated(
  roots: Iterable<Element | null>,
  { data = [], skip }: UntranslatedOptions = {},
): string[] {
  const found: string[] = [];
  // Whole texts first, the longest first, so a name pattern doesn't cut
  // into a text that is data as a whole.
  const patterns = [
    ...data
      .filter((pattern): pattern is string => typeof pattern === "string")
      .sort((a, b) => b.length - a.length),
    ...data.filter((pattern): pattern is RegExp => typeof pattern !== "string"),
  ];
  const check = (where: string, text: string) => {
    // Data before brackets: a value filled into a message stays inside its
    // brackets either way, and data may have brackets of its own (CSS).
    let rest = text;
    for (const pattern of patterns)
      rest = rest.replaceAll(
        typeof pattern === "string" || pattern.global
          ? pattern
          : new RegExp(pattern.source, `${pattern.flags}g`),
        "",
      );
    for (let before = ""; before !== rest; ) {
      before = rest;
      rest = rest.replace(/\[[^[\]]*\]/g, "");
    }
    if (english.test(rest)) found.push(`${where}: ${text.trim()}`);
  };
  for (const root of roots) {
    if (!root) continue;
    for (const element of [root, ...root.querySelectorAll("*")]) {
      if (skip && element.closest(skip)) continue;
      const name = element.getAttribute("data-part") ?? element.getAttribute("data-cs") ?? "";
      for (const attribute of attributes) {
        const value = element.getAttribute(attribute);
        if (value) check(`${element.tagName.toLowerCase()} ${name} [${attribute}]`, value);
      }
      for (const node of element.childNodes)
        if (node.nodeType === Node.TEXT_NODE && node.textContent)
          check(`${element.tagName.toLowerCase()} ${name}`, node.textContent);
    }
  }
  return found;
}
