/** The editor's Artwork tab (#175): the game's cover, background and icon.
 * Each can show one of the images automatic sources supplied, an image the
 * user imports (picker or drop), or none, and goes back to automatic with
 * Reset. Changes apply at once; refreshes never undo them. */
import { Button } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { EyeOff, ImagePlus, RotateCcw } from "lucide-react";
import { useState } from "react";
import {
  type ArtworkChange,
  type ArtworkImage,
  type ArtworkSlot,
  type EditableArtworkKind,
  useGameArtwork,
  useSetArtwork,
} from "../../api/gameEdit";
import { errorMessage } from "../../api/reply";
import { useDropZone } from "../../drop/DropProvider";
import { usePathPicker } from "../pathBrowser/PathPicker";
import type { GameEditorTabProps } from "./types";

const EXTENSIONS = ["png", "jpg", "jpeg", "webp"] as const;

export function ArtworkTab({ gameId }: GameEditorTabProps) {
  const { t } = useLingui();
  const slots = useGameArtwork(gameId);
  if (slots.isPending) return <p className="text-sm text-fg-muted">{t`Loading…`}</p>;
  if (slots.isError) return <p className="text-sm text-danger">{slots.error.message}</p>;
  if (!slots.data)
    return <p className="text-sm text-fg-muted">{t`This game is no longer in the library.`}</p>;
  return (
    <div data-part="artwork" className="flex flex-col gap-6">
      {slots.data.map((slot) => (
        <Slot key={slot.kind} gameId={gameId} slot={slot} />
      ))}
    </div>
  );
}

function Slot({ gameId, slot }: { gameId: string; slot: ArtworkSlot }) {
  const { t } = useLingui();
  const set = useSetArtwork(gameId);
  const picker = usePathPicker();
  const [error, setError] = useState<string | null>(null);
  const names: Record<EditableArtworkKind, string> = {
    cover: t`Cover`,
    hero: t`Background`,
    icon: t`Icon`,
  };
  const name = names[slot.kind];
  const apply = (change: ArtworkChange) => {
    setError(null);
    set.mutate({ kind: slot.kind, change }, { onError: (failure) => setError(failure.message) });
  };
  const importFrom = (path: string | null) => {
    if (path) apply({ op: "import", path });
  };
  const zone = useDropZone<HTMLElement>((paths) => importFrom(paths[0] ?? null));
  const chooseFile = () => {
    setError(null);
    picker
      .pick({ kind: "file", title: t`An image for the ${name}`, extensions: EXTENSIONS })
      .then(importFrom, (failure: unknown) => setError(errorMessage(failure)));
  };
  const status =
    slot.choice === "automatic"
      ? t`Automatic`
      : slot.choice === "chosen"
        ? t`Chosen by you`
        : t`None, by your choice`;
  return (
    <section
      ref={zone.ref}
      data-part="slot"
      data-kind={slot.kind}
      data-choice={slot.choice}
      data-drop-active={zone.active || undefined}
      aria-label={name}
      className={`flex flex-col gap-3 rounded-panel border p-4 ${zone.active ? "border-accent-alt" : "border-border"}`}
    >
      <div className="flex flex-wrap items-center gap-2">
        <h3 className="font-semibold">{name}</h3>
        <span data-part="choice" className="text-xs text-fg-muted">
          {status}
        </span>
      </div>
      <div className="flex flex-wrap items-start gap-4">
        <Preview image={slot.shown} kind={slot.kind} label={t`${name} shown now`} />
        <div className="flex min-w-0 flex-1 flex-col gap-3">
          {slot.automatic.length > 0 && (
            <div className="flex flex-col gap-1.5">
              <span className="text-xs text-fg-subtle">{t`From your sources`}</span>
              <ul className="flex flex-wrap gap-2">
                {slot.automatic.map((image, index) => {
                  const shown =
                    slot.shown !== null &&
                    (slot.choice === "automatic"
                      ? index === 0
                      : slot.shown.url !== null && slot.shown.url === image.url);
                  return (
                    <li key={image.id}>
                      <Button
                        data-part="candidate"
                        data-spatial="focusable"
                        data-selected={shown || undefined}
                        variant="ghost"
                        className={`h-auto p-1 ${shown ? "ring-2 ring-accent-alt" : ""}`}
                        aria-label={t`Use ${name} ${index + 1} from ${image.source}`}
                        aria-pressed={shown}
                        isDisabled={set.isPending}
                        onPress={() => apply({ op: "choose", artworkId: image.id })}
                      >
                        <Thumb image={image} kind={slot.kind} />
                      </Button>
                    </li>
                  );
                })}
              </ul>
            </div>
          )}
          <div className="flex flex-wrap gap-2">
            <Button
              data-part="import"
              data-spatial="focusable"
              icon={ImagePlus}
              size="sm"
              isDisabled={set.isPending}
              onPress={chooseFile}
            >
              <Trans>Import image…</Trans>
            </Button>
            {slot.choice !== "removed" && (
              <Button
                data-part="clear"
                data-spatial="focusable"
                icon={EyeOff}
                size="sm"
                variant="ghost"
                isDisabled={set.isPending}
                onPress={() => apply({ op: "remove" })}
              >
                <Trans>Show none</Trans>
              </Button>
            )}
            {slot.choice !== "automatic" && (
              <Button
                data-part="reset"
                data-spatial="focusable"
                icon={RotateCcw}
                size="sm"
                variant="ghost"
                isDisabled={set.isPending}
                aria-label={t`Reset ${name} to automatic`}
                onPress={() => apply({ op: "reset" })}
              >
                <Trans>Reset</Trans>
              </Button>
            )}
          </div>
          <p className="text-xs text-fg-subtle">
            <Trans>Or drop a PNG, JPEG or WebP image here.</Trans>
          </p>
          {error && (
            <p role="alert" data-part="error" className="text-xs text-danger">
              <Trans>The image wasn't changed: {error}</Trans>
            </p>
          )}
        </div>
      </div>
    </section>
  );
}

const shapes: Record<EditableArtworkKind, string> = {
  cover: "aspect-[2/3] w-32",
  hero: "aspect-video w-56",
  icon: "aspect-square w-16",
};

function Preview({
  image,
  kind,
  label,
}: {
  image: ArtworkImage | null;
  kind: EditableArtworkKind;
  label: string;
}) {
  return (
    <figure
      data-part="preview"
      aria-label={label}
      className={`grid shrink-0 place-items-center overflow-hidden rounded-control border border-border bg-surface-base/60 ${shapes[kind]}`}
    >
      <Thumb image={image} kind={kind} fill />
    </figure>
  );
}

function Thumb({
  image,
  kind,
  fill = false,
}: {
  image: ArtworkImage | null;
  kind: EditableArtworkKind;
  fill?: boolean;
}) {
  const { t } = useLingui();
  const size = fill
    ? "size-full"
    : kind === "hero"
      ? "h-16 w-28"
      : kind === "icon"
        ? "size-10"
        : "h-24 w-16";
  if (!image) return <span className="p-2 text-center text-xs text-fg-subtle">{t`None`}</span>;
  if (!image.url)
    return (
      <span
        className={`grid place-items-center bg-surface-overlay text-[10px] text-fg-subtle ${size}`}
      >
        {image.source === "user" ? t`Your image` : image.source}
      </span>
    );
  return <img src={image.url} alt="" className={`object-cover ${size}`} />;
}
