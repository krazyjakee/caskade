/** Open to (#114): the first view after Caskade starts, from
 * `core.general.startView`. `home` is the router's own `/`; `library` and
 * `collections` open those; `last` opens the view that was open when
 * Caskade last quit, remembered here on every navigation.
 *
 * It applies once, as the webview loads on `/`: a load anywhere else
 * (webview recovery puts the open page back) keeps its page, and console
 * mode keeps its own start. */
import { validReturn } from "../views/game/origin";

const KEY = "caskade.lastView";

/** The pages `last` can open: those a game can go back to, and Settings. */
function openable(href: unknown): string | undefined {
  if (typeof href !== "string") return undefined;
  if (/^\/settings(\?[^#]*)?$/.test(href)) return href;
  return validReturn(href);
}

/** Remembers `href` as the open view, for `last`. */
export function rememberView(href: string) {
  if (!openable(href)) return;
  try {
    localStorage.setItem(KEY, href);
  } catch {
    // Unavailable storage: `last` opens Home.
  }
}

export function rememberedView(): string | undefined {
  try {
    return openable(localStorage.getItem(KEY));
  } catch {
    return undefined;
  }
}

/** Where `view` opens, or nothing for Home (and anything unknown). */
export function startHref(view: unknown, last: string | undefined): string | undefined {
  switch (view) {
    case "library":
      return "/library";
    case "collections":
      return "/collections";
    case "last":
      return last === "/" ? undefined : last;
    default:
      return undefined;
  }
}

/** Where this load should open instead of the page it loaded on, if
 * anywhere. */
export async function startLocation({
  pathname,
  console,
  setting,
}: {
  /** The page the webview loaded on. */
  pathname: string;
  console: boolean;
  /** Reads `core.general.startView`. */
  setting: () => Promise<unknown>;
}): Promise<string | undefined> {
  if (console || pathname !== "/") return undefined;
  // Read before anything is remembered on this start.
  const last = rememberedView();
  try {
    return startHref(await setting(), last);
  } catch {
    // Unreadable settings: Home.
    return undefined;
  }
}
