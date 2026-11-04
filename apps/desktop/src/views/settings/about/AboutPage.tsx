import type { MessageDescriptor } from "@lingui/core";
import { msg } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { Link, useRouter } from "@tanstack/react-router";
import {
  Bug,
  ChevronRight,
  CircleArrowUp,
  ExternalLink as ExternalIcon,
  FileText,
  GitBranch,
  Package,
  Scale,
  ShieldCheck,
  Tag,
} from "lucide-react";
import { type LegalSection, project } from "./legal";
import { ExternalLink, SectionLink } from "./SectionLink";

/** The data services Caskade credits (#162): equal billing, side by side. */
const credits = [
  {
    name: "IGDB",
    host: "igdb.com",
    href: "https://www.igdb.com/",
    role: msg({ message: "Game metadata", context: "credit" }),
    description: msg`Game details, artwork and matches come from the Internet Game Database.`,
  },
  {
    name: "Hasheous",
    host: "hasheous.org",
    href: "https://hasheous.org/",
    role: msg({ message: "Game identification", context: "credit" }),
    description: msg`Hashes and titles that identify games and ROMs come from Hasheous.`,
  },
  {
    name: "Ludusavi manifest",
    host: "github.com/mtkennerly/ludusavi-manifest",
    href: "https://github.com/mtkennerly/ludusavi-manifest",
    role: msg({ message: "Save locations", context: "credit" }),
    description: msg`Save file and registry locations come from the Ludusavi manifest, an MIT-licensed project.`,
  },
  {
    name: "PCGamingWiki",
    host: "pcgamingwiki.com",
    href: "https://www.pcgamingwiki.com/",
    role: msg({ message: "Save location data", context: "credit" }),
    description: msg`The Ludusavi manifest includes data compiled from PCGamingWiki, licensed under CC BY-NC-SA 3.0.`,
  },
];

const projectLinks = [
  { label: msg`Project page`, href: project.home, icon: GitBranch },
  { label: msg`Report a problem`, href: project.issues, icon: Bug },
  {
    label: msg({ message: "Releases", context: "project link" }),
    href: project.releases,
    icon: Tag,
  },
];

const legalLinks: { section: LegalSection; label: MessageDescriptor; icon: typeof Scale }[] = [
  { section: "licenses", label: msg`Licenses`, icon: Scale },
  { section: "packages", label: msg`Open-source packages`, icon: Package },
  { section: "terms", label: msg`Terms and conditions`, icon: FileText },
  { section: "privacy", label: msg`Privacy policy`, icon: ShieldCheck },
];

const panel = "rounded-panel border border-border bg-surface-raised p-5";
const focusRing = "focus-visible:focus-ring";

/** Settings → About: the app and its build first (#68), credits, and the way
 * into Legal. */
export function AboutPage({ version, build }: { version: string; build: string }) {
  const { t } = useLingui();
  return (
    <div data-cs="about" className="flex min-w-0 flex-col gap-6">
      <h2 data-part="title" className="font-display text-2xl font-semibold tracking-wide">
        <Trans context="settings tab">About</Trans>
      </h2>
      <section data-part="details" aria-labelledby="about-caskade" className={panel}>
        <div className="flex items-start gap-4">
          <img src="/caskade.svg" alt="" className="size-12 shrink-0" />
          <div className="min-w-0">
            <h3 id="about-caskade" className="font-display text-xl font-semibold tracking-wide">
              Caskade
            </h3>
            <p className="mt-1 text-sm text-fg-muted">
              <Trans>
                A game library and launcher for Windows, Linux and macOS, with a desktop mode and a
                fullscreen console mode for gamepads.
              </Trans>
            </p>
          </div>
        </div>
        <dl data-part="version" className="mt-4 grid grid-cols-[auto_1fr] gap-x-6 gap-y-1 text-sm">
          <dt className="text-fg-subtle">
            <Trans>Version</Trans>
          </dt>
          <dd className="font-mono text-fg">{version}</dd>
          <dt className="text-fg-subtle">
            <Trans context="app build">Build</Trans>
          </dt>
          <dd className="text-fg">{build}</dd>
        </dl>
        <ul data-part="links" className="mt-4 flex flex-wrap gap-x-5 gap-y-2 text-sm">
          <UpdatesLink />
          {projectLinks.map(({ label, href, icon: Icon }) => (
            <li key={href}>
              <ExternalLink
                href={href}
                className={`inline-flex items-center gap-1.5 rounded-sm text-accent-alt underline-offset-2 hover:underline ${focusRing}`}
              >
                <Icon aria-hidden className="size-4" />
                {t(label)}
              </ExternalLink>
            </li>
          ))}
        </ul>
      </section>

      <ul data-part="credits" aria-label={t`Credits`} className="grid gap-4 sm:grid-cols-2">
        {credits.map((credit) => (
          <li key={credit.name} className="flex">
            <a
              data-part="credit"
              href={credit.href}
              target="_blank"
              rel="noreferrer"
              aria-label={t({
                message: `${{ role: t(credit.role) }} by ${{ name: credit.name }} (${{ host: credit.host }})`,
                comment: "A credit: 'Game metadata by IGDB (igdb.com)'",
              })}
              className={`flex flex-1 flex-col gap-1 rounded-panel border border-border bg-surface-raised p-5 transition-colors duration-(--cs-motion-duration-fast) hover:border-accent-alt/50 hover:bg-surface-hover ${focusRing}`}
            >
              <span className="font-mono text-xs tracking-wider text-fg-subtle uppercase">
                {t(credit.role)}
              </span>
              <span className="font-display text-2xl font-semibold tracking-wide text-fg">
                {credit.name}
              </span>
              <span className="text-sm text-fg-muted">{t(credit.description)}</span>
              <span className="mt-2 inline-flex items-center gap-1.5 text-sm text-accent-alt">
                {credit.host}
                <ExternalIcon aria-hidden className="size-3.5" />
              </span>
            </a>
          </li>
        ))}
      </ul>

      <nav data-part="legal-links" aria-labelledby="about-legal" className={panel}>
        <h3 id="about-legal" className="font-display text-lg font-semibold tracking-wide">
          <Trans>Legal</Trans>
        </h3>
        <p className="text-sm text-fg-muted">
          <Trans>Licenses, terms and privacy. They read offline.</Trans>
        </p>
        <ul className="mt-3 flex flex-col">
          {legalLinks.map(({ section, label, icon: Icon }) => (
            <li key={section} className="border-t border-border first:border-t-0">
              <SectionLink
                section={section}
                className={`flex items-center gap-3 rounded-control px-2 py-3 text-sm text-fg hover:bg-surface-hover ${focusRing}`}
              >
                <Icon aria-hidden className="size-4 shrink-0 text-fg-muted" />
                <span className="flex-1">{t(label)}</span>
                <ChevronRight aria-hidden className="size-4 shrink-0 text-fg-subtle" />
              </SectionLink>
            </li>
          ))}
        </ul>
      </nav>
    </div>
  );
}

/** To Caskade updates in General (#68); only where there is a router to follow it. */
function UpdatesLink() {
  const router = useRouter({ warn: false }) as ReturnType<typeof useRouter> | undefined;
  if (!router) return null;
  return (
    <li>
      <Link
        to="/settings"
        search={{ tab: "general" }}
        replace
        className={`inline-flex items-center gap-1.5 rounded-sm text-accent-alt underline-offset-2 hover:underline ${focusRing}`}
      >
        <CircleArrowUp aria-hidden className="size-4" />
        <Trans>Caskade updates</Trans>
      </Link>
    </li>
  );
}
