import { Trans, useLingui } from "@lingui/react/macro";
import { ChevronLeft, ChevronRight, X } from "lucide-react";
import { type PointerEvent, useEffect, useLayoutEffect, useRef, useState } from "react";
import { Button, Dialog, Heading, Modal, ModalOverlay } from "react-aria-components";
import type { Screenshot } from "../../api/types";
import { useInputAction } from "../../input";

/** How far a finger travels sideways before it counts as a swipe. */
const SWIPE = 48;

/** The row always holds this many slots, so the page keeps its height
 * while images download and when there are none. */
const RESERVED = 4;

/** A game's screenshots on its page; each opens the gallery on itself. */
export function Screenshots({
  title,
  screenshots: shots,
  loading = false,
}: {
  title: string;
  screenshots: Screenshot[];
  /** The page's images are still downloading. */
  loading?: boolean;
}) {
  const { t } = useLingui();
  const [open, setOpen] = useState<number | null>(null);
  const status = loading ? "loading" : "empty";
  const none = shots.length === 0;
  return (
    <div className="relative mt-4">
      <ul
        data-part="screenshots"
        aria-label={t`Screenshots`}
        aria-busy={loading && none}
        className="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-4"
      >
        {shots.map((shot, index) => {
          const number = index + 1;
          return (
            <li key={shot.full}>
              <Button
                onPress={() => setOpen(index)}
                className="relative block w-full overflow-hidden rounded-control border border-border outline-none data-focus-visible:focus-ring-over data-hovered:border-border-strong"
              >
                <img
                  src={shot.thumb}
                  alt={t({
                    message: `${title}, screenshot ${number}`,
                    comment: "{title} is the game's title",
                  })}
                  loading="lazy"
                  className="aspect-video w-full object-cover"
                />
              </Button>
            </li>
          );
        })}
        {Array.from({ length: Math.max(0, RESERVED - shots.length) }, (_, index) => (
          <li
            // biome-ignore lint/suspicious/noArrayIndexKey: empty slots have nothing else.
            key={index}
            aria-hidden="true"
            data-part="screenshot-slot"
            data-status={status}
            className="aspect-video w-full rounded-control border border-border bg-surface-raised data-[status=loading]:animate-wait data-[status=loading]:[animation-play-state:var(--cs-motion-ambient)]"
          />
        ))}
      </ul>
      {none && !loading && (
        <p
          data-part="screenshots-empty"
          className="pointer-events-none absolute inset-0 grid place-items-center text-sm text-fg-muted"
        >
          <Trans>No screenshots</Trans>
        </p>
      )}
      {open !== null && (
        <ScreenshotViewer title={title} shots={shots} start={open} close={() => setOpen(null)} />
      )}
    </div>
  );
}

/** The screenshots one at a time, filling the window, with a filmstrip.
 * Arrow keys, a swipe, a pad's left/right or LB/RB step through them
 * (round from the last to the first); B or Escape closes it. */
function ScreenshotViewer({
  title,
  shots,
  start,
  close,
}: {
  title: string;
  shots: Screenshot[];
  start: number;
  close: () => void;
}) {
  const { t } = useLingui();
  const count = shots.length;
  const [at, setAt] = useState(Math.min(Math.max(start, 0), count - 1));
  const dialog = useRef<HTMLElement>(null);
  const strip = useRef<HTMLOListElement>(null);
  const swipe = useRef<{ x: number; y: number } | null>(null);
  const go = (index: number) => setAt(((index % count) + count) % count);
  const shot = shots[at];

  // The filmstrip keeps the open screenshot in view, and focus on it
  // follows the screenshot.
  // biome-ignore lint/correctness/useExhaustiveDependencies: runs when the screenshot changes.
  useLayoutEffect(() => {
    const current = strip.current?.querySelector<HTMLElement>('[aria-current="true"]');
    if (!current) return;
    current.scrollIntoView?.({ block: "nearest", inline: "nearest" });
    const focused = document.activeElement;
    if (focused !== current && focused && strip.current?.contains(focused))
      current.focus({ preventScroll: true });
  }, [at]);

  // The neighbours load before they are asked for.
  useEffect(() => {
    if (count < 2) return;
    for (const by of [-1, 1]) {
      const near = shots[(at + by + count) % count];
      if (near) new Image().src = near.full;
    }
  }, [at, shots, count]);

  const many = count > 1;
  useInputAction("nav.left", () => go(at - 1), { owner: dialog, enabled: many });
  useInputAction("nav.right", () => go(at + 1), { owner: dialog, enabled: many });
  useInputAction("tab.prev", () => go(at - 1), {
    label: t({ message: "Previous", comment: "Goes to the previous screenshot" }),
    owner: dialog,
    enabled: many,
  });
  useInputAction("tab.next", () => go(at + 1), {
    label: t({ message: "Next", comment: "Goes to the next screenshot" }),
    owner: dialog,
    enabled: many,
  });

  // Keys while pad navigation is off; with it on, they arrive as the
  // actions above. React Aria's Dialog doesn't pass `onKeyDown` on.
  useEffect(() => {
    const element = dialog.current;
    if (!element || count < 2) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.altKey || event.ctrlKey || event.metaKey) return;
      const keys = ["ArrowLeft", "ArrowRight", "Home", "End"];
      if (!keys.includes(event.key)) return;
      event.preventDefault();
      setAt((from) => {
        if (event.key === "Home") return 0;
        if (event.key === "End") return count - 1;
        return (from + (event.key === "ArrowLeft" ? count - 1 : 1)) % count;
      });
    };
    element.addEventListener("keydown", onKeyDown);
    return () => element.removeEventListener("keydown", onKeyDown);
  }, [count]);
  const onPointerDown = (event: PointerEvent) => {
    swipe.current = event.pointerType === "mouse" ? null : { x: event.clientX, y: event.clientY };
  };
  const onPointerUp = (event: PointerEvent) => {
    const from = swipe.current;
    swipe.current = null;
    if (!from || !many) return;
    const dx = event.clientX - from.x;
    if (Math.abs(dx) > SWIPE && Math.abs(dx) > Math.abs(event.clientY - from.y))
      go(at + (dx < 0 ? 1 : -1));
  };

  if (!shot) return null;
  const number = at + 1;
  return (
    <ModalOverlay
      isOpen
      isDismissable
      onOpenChange={(isOpen) => {
        if (!isOpen) close();
      }}
      className="fixed inset-0 z-50 bg-surface-base/95 backdrop-blur-sm"
    >
      <Modal className="size-full">
        <Dialog
          ref={dialog}
          data-cs="screenshot-viewer"
          aria-label={t({
            message: `${title}, screenshots`,
            comment: "{title} is the game's title",
          })}
          className="flex size-full flex-col outline-none"
        >
          <header className="flex shrink-0 items-center gap-4 px-4 py-3 sm:px-6">
            <Heading
              slot="title"
              data-part="title"
              className="min-w-0 flex-1 truncate font-display text-lg font-semibold tracking-wide sm:text-xl"
            >
              {title}
            </Heading>
            <span
              data-part="counter"
              aria-live="polite"
              className="font-mono text-sm text-fg-muted tabular-nums"
            >
              <Trans comment="The screenshot shown, of how many">
                {number} / {count}
              </Trans>
            </span>
            <Button
              data-part="close"
              aria-label={t`Close`}
              onPress={close}
              className="grid size-10 place-items-center rounded-control text-fg-muted outline-none data-focus-visible:focus-ring data-hovered:bg-surface-hover data-hovered:text-fg"
            >
              <X className="size-5" />
            </Button>
          </header>
          <div
            data-part="stage"
            onPointerDown={onPointerDown}
            onPointerUp={onPointerUp}
            onPointerCancel={() => {
              swipe.current = null;
            }}
            className="relative min-h-0 flex-1 touch-pan-y select-none"
          >
            <img
              key={shot.full}
              data-part="image"
              src={shot.full}
              alt={t({
                message: `${title}, screenshot ${number} of ${count}`,
                comment: "{title} is the game's title",
              })}
              draggable={false}
              onError={(event) => {
                // No full-size copy: the thumbnail is better than nothing.
                if (event.currentTarget.src !== shot.thumb) event.currentTarget.src = shot.thumb;
              }}
              className="absolute inset-0 size-full object-contain p-2 sm:px-20 sm:py-4"
            />
            {many && (
              <>
                <Button
                  data-part="prev"
                  aria-label={t`Previous screenshot`}
                  onPress={() => go(at - 1)}
                  className="absolute top-1/2 left-2 grid size-10 -translate-y-1/2 place-items-center rounded-full border border-border bg-surface-raised/80 outline-none data-focus-visible:focus-ring data-hovered:bg-surface-hover sm:left-4 sm:size-12"
                >
                  <ChevronLeft className="size-6" />
                </Button>
                <Button
                  data-part="next"
                  aria-label={t`Next screenshot`}
                  onPress={() => go(at + 1)}
                  className="absolute top-1/2 right-2 grid size-10 -translate-y-1/2 place-items-center rounded-full border border-border bg-surface-raised/80 outline-none data-focus-visible:focus-ring data-hovered:bg-surface-hover sm:right-4 sm:size-12"
                >
                  <ChevronRight className="size-6" />
                </Button>
              </>
            )}
          </div>
          {many && (
            <ol
              ref={strip}
              data-part="strip"
              aria-label={t`All screenshots`}
              className="flex shrink-0 gap-2 overflow-x-auto px-4 py-3 sm:px-6 sm:py-4"
            >
              {shots.map((thumb, index) => {
                const number = index + 1;
                return (
                  <li key={thumb.full} className="shrink-0 first:ms-auto last:me-auto">
                    <Button
                      data-part="thumb"
                      data-selected={index === at || undefined}
                      data-focus-default={index === at || undefined}
                      aria-current={index === at}
                      aria-label={t`Screenshot ${number}`}
                      onPress={() => go(index)}
                      className="relative block overflow-hidden rounded-control border-2 border-transparent opacity-60 outline-none data-focus-visible:focus-ring-over data-focus-visible:opacity-100 data-hovered:opacity-100 data-selected:border-accent data-selected:opacity-100"
                    >
                      <img
                        src={thumb.thumb}
                        alt=""
                        loading="lazy"
                        draggable={false}
                        className="aspect-video h-14 object-cover sm:h-20"
                      />
                    </Button>
                  </li>
                );
              })}
            </ol>
          )}
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}
