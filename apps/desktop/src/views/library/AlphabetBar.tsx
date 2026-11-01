import { useLingui } from "@lingui/react/macro";
import { memo, useRef } from "react";
import { Button, Toolbar } from "react-aria-components";
import { compareTitles, LATIN } from "../../api/presentation";
import type { TitleLetter } from "../../api/types";

/** `#`, then A to Z and the language's other letters the games are filed
 * under (Å, Ä and Ö in Swedish), in the language's order. */
export function alphabetOf(letters: readonly TitleLetter[]): string[] {
  const more = letters.map(({ letter }) => letter).filter((l) => l !== "#" && !LATIN.includes(l));
  return ["#", ...[...LATIN, ...new Set(more)].sort(compareTitles)];
}

/** A letter per initial down the library's side, for jumping through a long
 * list sorted by title. Letters without games are disabled; dragging along
 * the bar jumps as the pointer passes each letter. */
export const AlphabetBar = memo(function AlphabetBar({
  letters,
  current,
  onJump,
}: {
  /** Where each letter's games start, as indexes into the library. */
  letters: readonly TitleLetter[];
  /** The letter of the games on screen. */
  current: string | null;
  onJump: (offset: number) => void;
}) {
  const { t } = useLingui();
  const offsets = new Map(letters.map(({ letter, offset }) => [letter, offset]));
  const alphabet = alphabetOf(letters);
  const dragged = useRef<string | null>(null);
  return (
    <Toolbar
      data-cs="alphabet-bar"
      aria-label={t`Jump to letter`}
      orientation="vertical"
      className="flex shrink-0 touch-none flex-col justify-between py-1 select-none"
      onPointerDown={() => {
        dragged.current = null;
      }}
      onPointerMove={(event) => {
        if (!(event.buttons & 1)) return;
        // Across the bar's middle, so the drag keeps working off its side.
        const bar = event.currentTarget.getBoundingClientRect();
        const letter = document
          .elementFromPoint(bar.left + bar.width / 2, event.clientY)
          ?.closest<HTMLElement>('[data-part="letter"]')?.dataset.letter;
        const offset = letter === undefined ? undefined : offsets.get(letter);
        if (letter === undefined || offset === undefined || letter === dragged.current) return;
        dragged.current = letter;
        onJump(offset);
      }}
    >
      {alphabet.map((letter) => {
        const offset = offsets.get(letter);
        return (
          <Button
            key={letter}
            data-part="letter"
            data-letter={letter}
            data-selected={letter === current || undefined}
            aria-current={letter === current || undefined}
            aria-label={letter === "#" ? t`Numbers and symbols` : letter}
            // A pad moves between the games; the bar is for the pointer.
            data-spatial="skip"
            isDisabled={offset === undefined}
            onPress={() => {
              if (offset !== undefined) onJump(offset);
            }}
            className="min-h-0 w-6 shrink rounded-control font-mono text-[11px] leading-4 text-fg-muted outline-none data-disabled:text-fg-subtle/40 data-focus-visible:focus-ring data-hovered:bg-surface-hover data-hovered:text-fg data-selected:text-accent-alt"
          >
            {letter}
          </Button>
        );
      })}
    </Toolbar>
  );
});
