import { useLingui } from "@lingui/react/macro";
import { Check } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Radio, RadioGroup, type RadioGroupProps } from "react-aria-components";
import { type AvatarInfo, avatarInfo, avatars as shipped } from "./avatars";

export type AvatarSize = "sm" | "md" | "lg" | "xl";

const sizes: Record<AvatarSize, string> = {
  sm: "size-9 text-xs",
  md: "size-11 text-sm",
  lg: "size-20 text-2xl",
  xl: "size-32 text-4xl",
};

/** Up to two letters from a name: "Ada Lovelace" → "AL", "kid" → "K". */
export function initials(name: string): string {
  const words = name.trim().split(/\s+/).filter(Boolean);
  const letters = (words.length > 1 ? [words[0], words.at(-1)] : words.slice(0, 1))
    .map((word) => Array.from(word ?? "")[0] ?? "")
    .join("");
  return letters.toLocaleUpperCase() || "?";
}

/** An account's picture: one of the shipped avatars, or the name's
 * initials when the id is unknown (an avatar a later release removed) or
 * the image can't load. Decorative unless given a `label`. */
export function Avatar({
  avatar,
  name,
  size = "md",
  label,
  part,
  className = "",
}: {
  /** An avatar id (`avatar:<name>`). */
  avatar: string | null | undefined;
  /** The account's name, for the initials. */
  name: string;
  size?: AvatarSize;
  /** Makes it an image with this accessible name. */
  label?: string;
  /** The `data-part` it plays in a parent component. */
  part?: string;
  className?: string;
}) {
  const info = avatarInfo(avatar);
  const [broken, setBroken] = useState<string | null>(null);
  const url = info && info.url !== broken ? info.url : null;
  return (
    <span
      data-cs="avatar"
      data-part={part}
      data-size={size}
      data-source={url ? "image" : "initials"}
      {...(label ? { role: "img", "aria-label": label } : { "aria-hidden": true })}
      className={`relative inline-grid shrink-0 place-items-center overflow-hidden rounded-full bg-linear-to-br from-accent-strong to-accent-alt font-display font-semibold text-accent-fg select-none ${sizes[size]} ${className}`}
    >
      {url ? (
        <img
          data-part="image"
          src={url}
          alt=""
          draggable={false}
          onError={() => setBroken(url)}
          className="absolute inset-0 size-full object-cover"
        />
      ) : (
        <span data-part="initials">{initials(name)}</span>
      )}
    </span>
  );
}

export interface AvatarPickerProps
  extends Omit<
    RadioGroupProps,
    "value" | "defaultValue" | "onChange" | "children" | "className" | "orientation"
  > {
  /** The chosen avatar's id. */
  value: string | null;
  onChange(id: string): void;
  /** Escape (B on a pad, through the app's keyboard map) leaves without a change. */
  onCancel?(): void;
  /** The avatars to offer; the shipped set by default. */
  options?: readonly AvatarInfo[];
  size?: Extract<AvatarSize, "md" | "lg" | "xl">;
  className?: string;
}

/** A grid of avatars to choose from. Mouse, keys (arrows move and choose,
 * as in any radio group) and pads (the app's spatial navigation moves, A
 * chooses) all work; it renders no focus logic of its own. */
export function AvatarPicker({
  value,
  onChange,
  onCancel,
  options = shipped,
  size = "lg",
  className = "",
  ...props
}: AvatarPickerProps) {
  const { t } = useLingui();
  const group = useRef<HTMLDivElement>(null);
  const cancel = useRef(onCancel);
  cancel.current = onCancel;
  useEffect(() => {
    const element = group.current;
    if (!element) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || !cancel.current) return;
      event.stopPropagation();
      cancel.current();
    };
    element.addEventListener("keydown", onKey);
    return () => element.removeEventListener("keydown", onKey);
  }, []);
  const labelled = Boolean(props["aria-label"] || props["aria-labelledby"]);
  return (
    <RadioGroup
      {...props}
      ref={group}
      aria-label={
        labelled
          ? props["aria-label"]
          : t({ message: "Avatar", comment: "The picker of an account's picture" })
      }
      data-cs="avatar-picker"
      orientation="horizontal"
      value={value}
      onChange={onChange}
      className={`grid grid-cols-[repeat(auto-fill,minmax(var(--avatar-cell,5.5rem),1fr))] gap-3 data-disabled:opacity-40 data-[size=xl]:[--avatar-cell:9rem] data-[size=md]:[--avatar-cell:3.5rem] ${className}`}
      data-size={size}
    >
      {options.map((option) => (
        <Radio
          key={option.id}
          value={option.id}
          aria-label={t(option.label)}
          data-part="option"
          className="group relative grid cursor-default place-items-center rounded-full p-1 outline-none transition-[background-color,box-shadow] duration-(--cs-motion-duration-fast) data-focus-visible:focus-ring data-hovered:bg-surface-hover data-selected:bg-accent/25 data-selected:shadow-[inset_0_0_0_2px] data-selected:shadow-accent-alt"
        >
          <Avatar avatar={option.id} name={t(option.label)} size={size} />
          <span
            data-part="check"
            aria-hidden
            className="absolute right-1 bottom-1 hidden size-6 place-items-center rounded-full bg-accent-alt text-accent-fg group-data-selected:grid"
          >
            <Check className="size-4" strokeWidth={3} />
          </span>
        </Radio>
      ))}
    </RadioGroup>
  );
}
