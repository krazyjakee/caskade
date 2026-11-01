import { useState } from "react";

export function artBackground(game: { art?: [number, number] }): string {
  if (!game.art)
    return "linear-gradient(145deg, var(--cs-color-surface-raised), var(--cs-color-surface-base))";
  const [a, b] = game.art;
  return `radial-gradient(120% 90% at 70% 10%, hsl(${b} 80% 55% / 0.55), transparent 60%),
    linear-gradient(160deg, hsl(${a} 55% 28%), hsl(${b} 60% 12%))`;
}

export function GameArt({
  game,
  className = "",
  image,
  part,
}: {
  game: { title: string; art?: [number, number] };
  className?: string;
  image?: string | null;
  /** The `data-part` it plays in a parent component, e.g. `art` in a card. */
  part?: string;
}) {
  // A cached file that's gone (the cache was cleared) shows the blank placeholder.
  const [broken, setBroken] = useState<string | null>(null);
  const shown = image && image !== broken ? image : null;
  return (
    <div
      data-cs="game-art"
      data-part={part}
      data-source={shown ? "image" : "placeholder"}
      aria-hidden="true"
      className={`relative overflow-hidden ${className}`}
      style={{ background: artBackground(game) }}
    >
      {shown && (
        <img
          data-part="image"
          src={shown}
          onError={() => setBroken(shown)}
          alt=""
          loading="lazy"
          decoding="async"
          className="absolute inset-0 size-full object-cover"
        />
      )}
    </div>
  );
}
