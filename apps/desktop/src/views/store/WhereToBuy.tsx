import { Trans } from "@lingui/react/macro";
import { ExternalLink } from "lucide-react";
import type { Ref } from "react";
import type { IgdbStoreLink } from "../../api/igdb";

/** The storefronts IGDB links the game to. A link says IGDB knows the
 * page, not that the game is on sale there or at what price. */
export function WhereToBuy({
  links,
  first,
}: {
  links: IgdbStoreLink[];
  first?: Ref<HTMLAnchorElement>;
}) {
  if (links.length === 0)
    return (
      <p data-part="no-stores" className="text-sm text-fg-muted">
        <Trans>IGDB lists no stores for this game.</Trans>
      </p>
    );
  return (
    <ul data-cs="store-links" className="flex flex-col gap-2">
      {links.map((link, i) => (
        <li key={link.url}>
          <a
            ref={i === 0 ? first : undefined}
            href={link.url}
            target="_blank"
            rel="noreferrer"
            data-part="store-link"
            data-store={link.store}
            className="flex items-center gap-2.5 rounded-control border border-border-strong bg-surface-overlay/70 px-4 py-2.5 text-sm transition-colors duration-(--cs-motion-duration-fast) hover:border-accent-alt/50 hover:bg-surface-hover focus-visible:focus-ring"
          >
            <span className="font-display font-semibold tracking-wide">
              {link.name || link.store}
            </span>
            {link.platforms.length > 0 && (
              <span className="truncate text-xs text-fg-subtle">{link.platforms.join(", ")}</span>
            )}
            <ExternalLink className="ml-auto size-3.5 shrink-0 text-fg-subtle" aria-hidden />
          </a>
        </li>
      ))}
    </ul>
  );
}
