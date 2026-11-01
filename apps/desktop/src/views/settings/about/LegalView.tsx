import { plural } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { ArrowLeft, ChevronDown } from "lucide-react";
import { type ReactNode, useId, useRef, useState } from "react";
import licensing from "../../../../../../docs/legal/LICENSING.md?raw";
import privacy from "../../../../../../docs/legal/PRIVACY.md?raw";
import terms from "../../../../../../docs/legal/TERMS.md?raw";
import { useDefaultFocus, useInputAction } from "../../../input";
import { scroller } from "../../../input/spatial";
import { type LegalSection, legalSections } from "./legal";
import { Markdown } from "./Markdown";
import notices from "./notices.generated.json";
import { SectionLink } from "./SectionLink";

const focusRing = "focus-visible:focus-ring";

/** A long text a pad scrolls through: up and down scroll it while it has
 * focus, then move on past its ends. Keys, wheels and touch scroll it natively. */
function ScrollDocument({ label, children }: { label: string; children: ReactNode }) {
  const { t } = useLingui();
  const ref = useRef<HTMLElement>(null);
  const step = (sign: 1 | -1) => {
    const article = ref.current;
    if (!article || document.activeElement !== article) return false;
    const box = scroller(article);
    if (!box) return false;
    const view = box.getBoundingClientRect();
    const rect = article.getBoundingClientRect();
    if (sign > 0 ? rect.bottom <= view.bottom + 1 : rect.top >= view.top - 1) return false;
    box.scrollBy?.({ top: sign * box.clientHeight * 0.4, behavior: "auto" });
    return true;
  };
  useInputAction("nav.down", () => step(1), {
    owner: ref,
    label: t({ message: "Scroll", context: "pad action" }),
  });
  useInputAction("nav.up", () => step(-1), { owner: ref });
  return (
    <article
      ref={ref}
      data-part="document"
      aria-label={label}
      // A stop of its own, so a pad and the keyboard can scroll it.
      // biome-ignore lint/a11y/noNoninteractiveTabindex: a scrollable document takes focus.
      tabIndex={0}
      className={`min-w-0 rounded-panel border border-border bg-surface-raised p-5 break-words sm:p-6 ${focusRing}`}
    >
      {children}
    </article>
  );
}

function LicenseText({ title, text }: { title: string; text: string }) {
  return (
    <>
      <h4 className="mt-6 text-base font-semibold text-fg">{title}</h4>
      <pre className="mt-2 rounded-control bg-surface-base/50 px-3 py-2 font-mono text-xs leading-relaxed whitespace-pre-wrap break-words text-fg-muted">
        {text}
      </pre>
    </>
  );
}

/** Caskade's licensing and 7-Zip's licences, in English as published; only
 * their headings are translated. */
function Licenses() {
  const { t } = useLingui();
  const { sevenZip } = notices;
  return (
    <ScrollDocument label={t`Licenses`}>
      <Markdown source={licensing} />
      {sevenZip.licenses.map((licence) => (
        <LicenseText
          key={licence.platforms.join()}
          title={t`7-Zip ${{ version: sevenZip.version }} licence (${{ platforms: licence.platforms.join(", ") }})`}
          text={licence.text}
        />
      ))}
    </ScrollDocument>
  );
}

const texts: Readonly<Record<string, string>> = notices.texts;

/** The licences an expression names: `(MIT OR Apache-2.0) AND Unicode-3.0` → three. */
function licenceTerms(expression: string) {
  return expression
    .replace(/[()]/g, " ")
    .split(/\s+(?:OR|AND)\s+/)
    .map((term) => term.trim())
    .filter(Boolean);
}

type Group = (typeof notices.groups)[number];

function PackageGroup({ group }: { group: Group }) {
  const { t } = useLingui();
  const [open, setOpen] = useState(false);
  const body = useId();
  const count = group.packages.length;
  return (
    <li className="rounded-control border border-border bg-surface-raised/60">
      <button
        type="button"
        aria-expanded={open}
        aria-controls={body}
        onClick={() => setOpen(!open)}
        className={`flex w-full items-center gap-3 rounded-control px-4 py-3 text-left hover:bg-surface-hover ${focusRing}`}
      >
        <span className="min-w-0 flex-1 font-mono text-sm break-words text-fg">
          {group.license}
        </span>
        <span className="shrink-0 text-xs text-fg-subtle">
          {t({ message: plural(count, { one: "# package", other: "# packages" }) })}
        </span>
        <ChevronDown
          aria-hidden
          className={`size-4 shrink-0 text-fg-muted transition-transform ${open ? "rotate-180" : ""}`}
        />
      </button>
      {open && (
        <div id={body} className="border-t border-border p-2">
          <ScrollDocument label={t`Packages under ${{ license: group.license }}`}>
            <ul className="flex flex-col gap-3 text-sm">
              {group.packages.map((item) => (
                <li key={`${item.ecosystem}:${item.name}@${item.version}`} className="min-w-0">
                  <span className="font-mono text-fg">
                    {item.name} {item.version}
                  </span>
                  <span className="text-xs text-fg-subtle">
                    {" "}
                    · {item.ecosystem === "rust" ? t`Rust crate` : t`JavaScript package`}
                  </span>
                  {item.copyright.length > 0 && (
                    <span className="block text-xs text-fg-muted">
                      {item.copyright.join(" · ")}
                    </span>
                  )}
                  {item.repository && (
                    <span className="block font-mono text-xs break-all text-fg-subtle">
                      {item.repository}
                    </span>
                  )}
                  {"notice" in item && typeof item.notice === "string" && (
                    <pre className="mt-1 font-mono text-xs whitespace-pre-wrap break-words text-fg-muted">
                      {item.notice}
                    </pre>
                  )}
                </li>
              ))}
            </ul>
            {licenceTerms(group.license).map((term) => {
              const text = texts[term];
              return text ? (
                <LicenseText key={term} title={term} text={text} />
              ) : (
                <p key={term} className="mt-6 text-sm text-fg-muted">
                  {group.license.includes(" OR ")
                    ? t`${{ term }} is one of the choices these packages offer; Caskade uses another, so its text isn't bundled.`
                    : t`${{ term }}: its text isn't bundled; see the packages' repositories.`}
                </p>
              );
            })}
          </ScrollDocument>
        </div>
      )}
    </li>
  );
}

function Packages() {
  const { t } = useLingui();
  const all = notices.groups.flatMap((group) => group.packages);
  const rust = all.filter((item) => item.ecosystem === "rust").length;
  return (
    <div className="flex min-w-0 flex-col gap-4">
      <div className="text-sm leading-relaxed text-fg-muted">
        <h3 className="font-display text-2xl font-semibold tracking-wide text-fg">
          <Trans>Open-source packages</Trans>
        </h3>
        <p className="mt-3">
          {t`Caskade is built with ${{ crates: rust }} Rust crates and ${{ packages: all.length - rust }} JavaScript packages, grouped here by licence. Open a licence for its packages, their copyright holders and the licence's text. Where a licence reads "A OR B", the package may be used under either.`}
        </p>
      </div>
      <ul aria-label={t`Licences`} className="flex flex-col gap-2">
        {notices.groups.map((group) => (
          <PackageGroup key={group.license} group={group} />
        ))}
      </ul>
    </div>
  );
}

/** Published in English: only the document's label is translated. */
function Terms() {
  const { t } = useLingui();
  return (
    <ScrollDocument label={t`Terms and conditions`}>
      <Markdown source={terms} />
    </ScrollDocument>
  );
}

function Privacy() {
  const { t } = useLingui();
  return (
    <ScrollDocument label={t`Privacy policy`}>
      <Markdown source={privacy} />
    </ScrollDocument>
  );
}

const documents: Record<LegalSection, () => ReactNode> = {
  licenses: Licenses,
  packages: Packages,
  terms: Terms,
  privacy: Privacy,
};

/** Settings → About → Legal (#162): Caskade's licences, the open-source
 * packages it ships, its terms and its privacy policy, bundled so they read
 * offline. Each section has an address of its own. */
export function LegalView({ section }: { section: LegalSection }) {
  const { t } = useLingui();
  const Document = documents[section];
  // The link that opened this view is gone: a pad goes on from the open section.
  const current = useRef<HTMLAnchorElement>(null);
  useDefaultFocus(current, true, section);
  return (
    <div data-cs="legal" className="flex min-w-0 flex-col gap-4">
      <SectionLink
        section={null}
        data-part="back"
        className={`inline-flex w-fit items-center gap-1.5 rounded-control px-2 py-1 text-sm text-fg-muted hover:bg-surface-hover hover:text-fg ${focusRing}`}
      >
        <ArrowLeft aria-hidden className="size-4" />
        <Trans>Back to About</Trans>
      </SectionLink>
      <h2 data-part="title" className="font-display text-2xl font-semibold tracking-wide">
        <Trans>Legal</Trans>
      </h2>
      <nav data-part="nav" aria-label={t`Legal sections`}>
        <ul className="flex flex-wrap gap-2">
          {legalSections.map(({ id, label }) => (
            <li key={id}>
              <SectionLink
                section={id}
                ref={id === section ? current : undefined}
                data-part="section-link"
                aria-current={id === section ? "page" : undefined}
                className={`block rounded-control border border-border px-3 py-1.5 text-sm text-fg-muted hover:bg-surface-hover hover:text-fg aria-[current=page]:border-accent-alt/50 aria-[current=page]:bg-accent/20 aria-[current=page]:text-fg ${focusRing}`}
              >
                {t(label)}
              </SectionLink>
            </li>
          ))}
        </ul>
      </nav>
      <Document />
    </div>
  );
}

export default LegalView;
