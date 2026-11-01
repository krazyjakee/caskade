import { Trans, useLingui } from "@lingui/react/macro";
import { ChevronDown, Ellipsis, Play } from "lucide-react";
import type { ReactNode } from "react";
import { type GameStatus, StatusBadge } from "./StatusBadge";

/** What the hero shows; the app's game types fit it. */
export interface HeroGame {
  title: string;
  genre: string;
  status: GameStatus;
  /** Hue pair for the background. */
  art: [number, number];
  description?: string;
  playtimeHours?: number;
  lastPlayed?: string;
  sizeGb?: number;
}

export function Hero({
  game,
  eyebrow,
  action,
  actions,
}: {
  game: HeroGame;
  eyebrow?: string;
  /** Label of the launch button. */
  action?: string;
  /** Replaces the launch button and its neighbours, e.g. with store links. */
  actions?: ReactNode;
}) {
  const { t } = useLingui();
  const [a, b] = game.art;
  const { sizeGb, playtimeHours, lastPlayed } = game;
  return (
    <section
      data-cs="hero"
      className="relative flex h-(--cs-hero-height) flex-col justify-center overflow-hidden rounded-panel border border-border px-10"
      style={{ background: `linear-gradient(135deg, hsl(${a} 50% 20%), hsl(${b} 55% 10%))` }}
    >
      <div
        data-part="glow"
        aria-hidden
        className="absolute -inset-1/4"
        style={{
          background: `radial-gradient(40% 50% at 70% 40%, hsl(${b} 70% 50% / 0.55), transparent 70%)`,
        }}
      />
      <div
        data-part="shade"
        aria-hidden
        className="absolute inset-0 bg-linear-to-r from-surface-base from-25% to-transparent to-70%"
      />
      <div data-part="fx" aria-hidden />

      <div data-part="content" className="relative">
        {eyebrow && (
          <div
            data-part="eyebrow"
            className="mb-3 flex animate-enter items-center gap-2 font-mono text-xs tracking-[0.2em] text-accent-alt uppercase"
          >
            <span
              data-part="live-dot"
              className="size-1.5 rounded-full bg-accent-alt shadow-[0_0_8px] shadow-accent-alt"
            />
            {eyebrow}
          </div>
        )}
        <h1
          data-part="title"
          className="animate-enter font-display text-5xl font-semibold tracking-[0.14em] uppercase [--i:1]"
        >
          {game.title}
        </h1>
        {game.description && (
          <p
            data-part="description"
            className="mt-4 max-w-md animate-enter text-sm leading-relaxed text-fg-muted [--i:2]"
          >
            {game.description}
          </p>
        )}
        <div
          data-part="meta"
          className="mt-3 flex animate-enter gap-2 font-mono text-[11px] tracking-wide text-fg-muted uppercase [--i:3]"
        >
          {[
            game.genre,
            sizeGb !== undefined &&
              !playtimeHours &&
              t({ message: `${sizeGb} GB`, comment: "A game's size on disk, in gigabytes" }),
            playtimeHours !== undefined &&
              t({ message: `${playtimeHours}h played`, comment: "Hours played" }),
            lastPlayed && t`Last played ${lastPlayed}`,
          ]
            .filter(Boolean)
            .map((item, i) => (
              <span key={String(item)} className="flex gap-2">
                {i > 0 && <span className="text-accent-alt/60">{"//"}</span>}
                {item}
              </span>
            ))}
        </div>
        <div data-part="actions" className="mt-6 flex animate-enter items-center gap-3 [--i:4]">
          {actions ?? (
            <>
              <div data-part="primary-action">
                <div className="chamfer flex bg-linear-to-r from-accent to-accent-strong text-accent-fg">
                  <button
                    type="button"
                    className="flex items-center gap-2 px-6 py-2.5 font-display font-semibold tracking-wider uppercase hover:bg-white/10"
                  >
                    <Play className="size-4 fill-current" />
                    {action}
                  </button>
                  <button
                    type="button"
                    aria-label={t`More launch options`}
                    className="border-l border-white/20 px-2.5 hover:bg-white/10"
                  >
                    <ChevronDown className="size-4" />
                  </button>
                </div>
              </div>
              <button
                type="button"
                className="rounded-control border border-border-strong bg-surface-overlay/70 px-5 py-2.5 text-sm backdrop-blur hover:border-accent-alt/50 hover:bg-surface-hover"
              >
                <Trans>View Details</Trans>
              </button>
              <button
                type="button"
                aria-label={t({ message: "More", comment: "A button opening more actions" })}
                className="rounded-control border border-border-strong bg-surface-overlay/70 p-2.5 backdrop-blur hover:border-accent-alt/50 hover:bg-surface-hover"
              >
                <Ellipsis className="size-4" />
              </button>
            </>
          )}
        </div>
        <div className="mt-4 animate-enter [--i:5]">
          <StatusBadge part="badge" status={game.status} />
        </div>
      </div>
    </section>
  );
}
