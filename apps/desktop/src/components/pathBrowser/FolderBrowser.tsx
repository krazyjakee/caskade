/** Console mode's file and folder picker: native dialogs can't be driven
 * with a pad, and Steam Deck's game mode has none. A pad moves through the
 * entries, A opens a folder or picks a file, B cancels, LB/RB go to the
 * previous and next place, LT/RT page through a long folder and Start
 * picks the folder shown. Touch and the mouse work the same way. */
import { Button } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { ArrowUp, File, Folder, HardDrive, Home } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Button as AriaButton, Dialog, Heading, Modal, ModalOverlay } from "react-aria-components";
import type { BrowseApi, DirEntry, DirListing, Place } from "../../api/browse";
import { formatPath } from "../../api/presentation";
import { errorMessage } from "../../api/reply";
import { useInputAction } from "../../input";

export interface PathRequest {
  kind: "file" | "folder";
  title: string;
  /** File mode: the extensions that may be picked (without the dot); other
   * files show dimmed. Every file without it. */
  extensions?: readonly string[];
  /** Where to start; home without it. */
  startAt?: string | null;
}

/** The place `path` is in: the one with the longest matching path. */
export function currentPlace(places: readonly Place[], path: string): number {
  let best = -1;
  let length = -1;
  places.forEach((place, index) => {
    const base = place.path.replace(/[\\/]+$/, "");
    const inside =
      path === place.path ||
      path === base ||
      path.startsWith(`${base}/`) ||
      path.startsWith(`${base}\\`);
    if (inside && base.length > length) {
      best = index;
      length = base.length;
    }
  });
  return best;
}

function pickable(entry: DirEntry, request: PathRequest): boolean {
  if (entry.isDir) return true;
  if (request.kind === "folder") return false;
  if (!request.extensions?.length) return true;
  const name = entry.name.toLowerCase();
  return request.extensions.some((extension) =>
    name.endsWith(`.${extension.toLowerCase().replace(/^\./, "")}`),
  );
}

const row =
  "flex min-h-12 w-full items-center gap-3 rounded-control px-4 py-2 text-left text-base outline-none transition-colors duration-(--cs-motion-duration-fast) data-hovered:bg-surface-hover data-pressed:bg-surface-hover data-focus-visible:bg-accent/10 data-focus-visible:focus-ring data-disabled:cursor-default data-disabled:opacity-40";

export function FolderBrowser({
  request,
  api,
  onClose,
}: {
  request: PathRequest;
  api: BrowseApi;
  /** The path picked, or null when cancelled. */
  onClose: (path: string | null) => void;
}) {
  const { t } = useLingui();
  const dialog = useRef<HTMLElement>(null);
  const list = useRef<HTMLElement>(null);
  // `from`: the folder just left by going up, to land on again.
  const [at, setAt] = useState<{ path: string | null; from?: string }>({
    path: request.startAt ?? null,
  });
  const [shown, setShown] = useState<{ listing: DirListing; from?: string } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const listing = shown?.listing ?? null;
  const loaded = useRef(false);

  useEffect(() => {
    let gone = false;
    api.list(at.path).then(
      (next) => {
        if (gone) return;
        loaded.current = true;
        setShown({ listing: next, from: at.from });
        setError(null);
      },
      (reason: unknown) => {
        if (gone) return;
        setError(errorMessage(reason));
        // A start folder that has gone: start at home instead.
        if (at.path !== null && !loaded.current) setAt({ path: null });
      },
    );
    return () => {
      gone = true;
    };
  }, [api, at]);

  // Focus lands on the first entry when a folder opens (on the folder just
  // left, going up), never on the dialog: A must do something. The entry
  // is marked `data-focus-default`, so a dialog that took focus itself
  // hands it on too.
  const land = useCallback(() => {
    const target =
      dialog.current?.querySelector<HTMLElement>("[data-focus-default]") ??
      dialog.current?.querySelector<HTMLElement>('[data-part="select"]');
    if (!target) return;
    target.focus({ preventScroll: true, focusVisible: true } as FocusOptions);
    target.scrollIntoView?.({ block: "nearest" });
  }, []);
  useEffect(() => {
    if (shown) land();
  }, [shown, land]);
  useEffect(() => {
    const element = dialog.current;
    if (!element) return;
    const onFocus = (event: FocusEvent) => {
      if (event.target === element && loaded.current) land();
    };
    element.addEventListener("focus", onFocus);
    return () => element.removeEventListener("focus", onFocus);
  }, [land]);

  const open = useCallback((path: string, from?: string) => setAt({ path, from }), []);
  const places = listing?.places ?? [];
  const placeIndex = listing ? currentPlace(places, listing.path) : -1;
  const step = (by: number) => {
    if (!places.length) return false;
    const index =
      placeIndex < 0
        ? by > 0
          ? 0
          : places.length - 1
        : (placeIndex + by + places.length) % places.length;
    const place = places[index];
    if (place) open(place.path);
    return true;
  };
  const page = (sign: 1 | -1) => {
    const box = list.current;
    if (!box) return false;
    const enabled = [...box.querySelectorAll<HTMLButtonElement>("button")].filter(
      (element) => !element.disabled,
    );
    if (!enabled.length) return true;
    const active = document.activeElement;
    const current = enabled.find((element) => element === active) ?? enabled[0];
    if (!current) return true;
    const top = (element: Element) => element.getBoundingClientRect().top;
    const height = current.getBoundingClientRect().height;
    const distance = Math.max(box.clientHeight * 0.9, height);
    const from = top(current);
    const target =
      sign > 0
        ? (enabled.find((element) => top(element) >= from + distance) ?? enabled.at(-1))
        : (enabled.findLast((element) => top(element) <= from - distance) ?? enabled[0]);
    if (!target) return true;
    target.focus({ preventScroll: true, focusVisible: true } as FocusOptions);
    target.scrollIntoView?.({ block: "start" });
    return true;
  };
  const folderMode = request.kind === "folder";
  useInputAction("tab.prev", () => step(-1), { label: t`Previous place`, owner: dialog });
  useInputAction("tab.next", () => step(1), { label: t`Next place`, owner: dialog });
  useInputAction("page.prev", () => page(-1), { label: t`Page up`, owner: dialog });
  useInputAction("page.next", () => page(1), { label: t`Page down`, owner: dialog });
  useInputAction(
    "menu",
    () => {
      if (listing) onClose(listing.path);
    },
    { label: t`Select folder`, owner: dialog, enabled: folderMode && listing !== null },
  );

  const entries = useMemo(
    () => (listing?.entries ?? []).map((entry) => ({ entry, enabled: pickable(entry, request) })),
    [listing, request],
  );

  const landing =
    (shown?.from && entries.some(({ entry }) => entry.path === shown.from) && shown.from) ||
    entries.find(({ enabled }) => enabled)?.entry.path ||
    (listing?.parent ? "up" : null);

  return (
    <ModalOverlay
      isOpen
      isDismissable
      onOpenChange={(isOpen) => {
        if (!isOpen) onClose(null);
      }}
      className="fixed inset-0 z-50 grid place-items-center bg-surface-base/80 p-4 backdrop-blur-sm"
    >
      <Modal className="flex h-[85vh] w-full max-w-3xl flex-col rounded-panel border border-border-strong bg-surface-raised shadow-lg">
        <Dialog
          ref={dialog}
          data-cs="folder-browser"
          data-kind={request.kind}
          className="flex min-h-0 flex-1 flex-col gap-4 p-6 outline-none"
        >
          <div>
            <Heading slot="title" data-part="title" className="font-display text-2xl font-semibold">
              {request.title}
            </Heading>
            <p data-part="location" className="mt-1 break-all font-mono text-base text-fg-muted">
              {listing ? formatPath(listing.path) : "…"}
            </p>
          </div>
          {places.length > 0 && (
            <nav
              data-part="places"
              aria-label={t({ message: "Places", context: "folder browser" })}
              className="flex flex-wrap gap-2"
            >
              {places.map((place, index) => {
                const Icon = index === 0 ? Home : HardDrive;
                return (
                  <AriaButton
                    key={place.path}
                    data-part="place"
                    data-status={index === placeIndex ? "active" : undefined}
                    aria-current={index === placeIndex ? "true" : undefined}
                    onPress={() => open(place.path)}
                    className="flex min-h-11 items-center gap-2 rounded-control border border-border px-4 text-base outline-none data-hovered:bg-surface-hover data-focus-visible:focus-ring aria-[current]:border-accent-alt aria-[current]:bg-accent/10"
                  >
                    <Icon aria-hidden className="size-5 text-fg-muted" />
                    {place.label}
                  </AriaButton>
                );
              })}
            </nav>
          )}
          {error && (
            <p data-part="error" role="alert" className="text-base text-danger">
              {error}
            </p>
          )}
          <section
            ref={list}
            data-part="entries"
            aria-label={t`Folder contents`}
            className="min-h-0 flex-1 overflow-y-auto rounded-control border border-border bg-surface-base/40 p-1"
          >
            {listing?.parent && (
              <AriaButton
                data-part="up"
                data-focus-default={landing === "up" || undefined}
                onPress={() => listing.parent && open(listing.parent, listing.path)}
                className={row}
              >
                <ArrowUp aria-hidden className="size-6 shrink-0 text-fg-muted" />
                <span className="font-medium">
                  <Trans>Up one folder</Trans>
                </span>
              </AriaButton>
            )}
            {entries.map(({ entry, enabled }) => {
              const Icon = entry.isDir ? Folder : File;
              return (
                <AriaButton
                  key={entry.path}
                  data-part="entry"
                  data-focus-default={landing === entry.path || undefined}
                  data-kind={entry.isDir ? "folder" : "file"}
                  isDisabled={!enabled}
                  onPress={() => (entry.isDir ? open(entry.path) : onClose(entry.path))}
                  className={row}
                >
                  <Icon
                    data-part="icon"
                    aria-hidden
                    className={`size-6 shrink-0 ${entry.isDir ? "text-accent-alt" : "text-fg-muted"}`}
                  />
                  <span data-part="name" className="min-w-0 truncate">
                    {entry.name}
                  </span>
                </AriaButton>
              );
            })}
            {listing && entries.length === 0 && (
              <p data-part="empty" className="px-4 py-3 text-base text-fg-muted">
                <Trans>This folder is empty.</Trans>
              </p>
            )}
          </section>
          <div data-part="actions" className="flex justify-end gap-3">
            <Button autoFocus={!listing} onPress={() => onClose(null)}>
              <Trans>Cancel</Trans>
            </Button>
            {folderMode && (
              <Button
                variant="primary"
                data-part="select"
                isDisabled={!listing}
                onPress={() => listing && onClose(listing.path)}
              >
                <Trans>Select this folder</Trans>
              </Button>
            )}
          </div>
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}
