import { type ComponentType, createContext, type ReactNode, type Ref, useContext } from "react";
import type { LegalSection } from "./legal";

export interface SectionLinkProps {
  /** A Legal section, or `null` for About itself. */
  section: LegalSection | null;
  children: ReactNode;
  className?: string;
  "data-part"?: string;
  "aria-current"?: "page";
  ref?: Ref<HTMLAnchorElement>;
}

/** A plain link to the section's address: Storybook and tests without a router. */
function HrefLink({ section, ...props }: SectionLinkProps) {
  return <a href={`?tab=about${section ? `&section=${section}` : ""}`} {...props} />;
}

/** How About and the Legal view link to each other: the app gives a router link. */
export const SectionLinkContext = createContext<ComponentType<SectionLinkProps>>(HrefLink);

export function SectionLink(props: SectionLinkProps) {
  const Link = useContext(SectionLinkContext);
  return <Link {...props} />;
}

/** Styling for a link that reads as text: accent, underlined on hover, a focus ring. */
export const textLink =
  "rounded-sm text-accent-alt underline-offset-2 hover:underline focus-visible:focus-ring";

export function ExternalLink({
  href,
  children,
  className = textLink,
}: {
  href: string;
  children: ReactNode;
  className?: string;
}) {
  return (
    <a href={href} target="_blank" rel="noreferrer" className={className}>
      {children}
    </a>
  );
}
