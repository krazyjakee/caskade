/** Modules the Vite plugin (`@caskade/i18n/vite`) makes. */

declare module "virtual:caskade-i18n/catalogs" {
  import type { Messages } from "@lingui/core";

  type Loader = () => Promise<{ messages: Messages }>;
  /** The pseudo-locale, when this build offers it. */
  export const pseudoLocale: string | undefined;
  /** A lazy loader per language and domain; each is a chunk of its own. */
  export const catalogs: Record<string, Partial<Record<"ui" | "app", Loader>>>;
  /** How much of each language is translated, from 0 to 1 (#213). */
  export const completeness: Record<string, number>;
}

/** A PO file imported directly, compiled on its own (fuzzy entries left out). */
declare module "*.po" {
  import type { Messages } from "@lingui/core";

  export const messages: Messages;
}
