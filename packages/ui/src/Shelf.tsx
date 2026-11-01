import { Trans } from "@lingui/react/macro";
import type { LucideIcon } from "lucide-react";
import { ArrowRight } from "lucide-react";
import type { ReactNode } from "react";

export function Shelf({
  title,
  icon: Icon,
  subtitle,
  children,
}: {
  title: string;
  icon: LucideIcon;
  subtitle?: string;
  children: ReactNode;
}) {
  return (
    <section data-cs="shelf" className="mt-8">
      <header data-part="header" className="mb-3 flex items-center gap-3">
        <Icon data-part="icon" className="size-5 text-accent-alt" />
        <h2 data-part="title" className="font-display text-lg font-semibold tracking-wide">
          {title}
        </h2>
        {subtitle && (
          <span data-part="subtitle" className="text-sm text-fg-subtle">
            {subtitle}
          </span>
        )}
        <span
          aria-hidden
          className="h-px flex-1 bg-linear-to-r from-border-strong to-transparent"
        />
        <button
          type="button"
          data-part="view-all"
          className="group flex items-center gap-1 font-mono text-xs tracking-wider text-accent-alt uppercase"
        >
          <Trans>View All</Trans>
          <ArrowRight className="size-4 transition-transform duration-(--cs-motion-duration-fast) group-hover:translate-x-1" />
        </button>
      </header>
      <div data-part="items" className="grid grid-cols-[repeat(auto-fill,minmax(11rem,1fr))] gap-4">
        {children}
      </div>
    </section>
  );
}
