import { msg } from "@lingui/core/macro";

/** The Legal view's sections (#162), each at `/settings?tab=about&section=<id>`.
 * Their names are translated (`t(label)`); the documents stay as published. */
export const legalSections = [
  { id: "licenses", label: msg`Licenses` },
  { id: "packages", label: msg`Open-source packages` },
  { id: "terms", label: msg`Terms and conditions` },
  { id: "privacy", label: msg`Privacy policy` },
] as const;
export type LegalSection = (typeof legalSections)[number]["id"];

/** `value` when it names a Legal section; About itself otherwise. */
export function legalSection(value: unknown): LegalSection | undefined {
  return legalSections.find((section) => section.id === value)?.id;
}

/** The documents in `docs/legal/`, by file name: a link between them opens its section. */
export const legalDocuments: Readonly<Record<string, LegalSection>> = {
  "LICENSING.md": "licenses",
  "TERMS.md": "terms",
  "PRIVACY.md": "privacy",
};

export const project = {
  home: "https://github.com/krazyjakee/caskade",
  issues: "https://github.com/krazyjakee/caskade/issues",
  releases: "https://github.com/krazyjakee/caskade/releases",
};
