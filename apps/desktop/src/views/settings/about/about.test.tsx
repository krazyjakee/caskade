/** #162: Settings → About, with the IGDB and Hasheous credits, and the Legal
 * view with the licences, packages, terms and privacy policy it bundles. */
import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import appConfig from "../../../../src-tauri/tauri.conf.json";
import { renderApp } from "../../../test/harness";
import { legalSection, legalSections } from "./legal";
import { parseMarkdown } from "./Markdown";
import notices from "./notices.generated.json";

afterEach(cleanup);

const follows = (a: Node, b: Node) =>
  Boolean(a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING);

it.each(["desktop", "console"] as const)(
  "credits IGDB and Hasheous first, then the app, its version and links (%s mode)",
  async (mode) => {
    renderApp("/settings?tab=about", { mode });
    expect(await screen.findByRole("tab", { name: "About", selected: true })).toBeTruthy();
    const heading = await screen.findByRole("heading", { name: "About" });
    const credits = screen.getByRole("list", { name: "Credits" });
    const links = within(credits).getAllByRole("link");
    expect(links.map((link) => link.getAttribute("href"))).toEqual([
      "https://www.igdb.com/",
      "https://hasheous.org/",
    ]);
    expect(links.map((link) => link.getAttribute("aria-label"))).toEqual([
      "Game metadata by IGDB (igdb.com)",
      "Game identification by Hasheous (hasheous.org)",
    ]);
    // The same weight: one style for both.
    expect(links[0]?.className).toBe(links[1]?.className);
    // Right under the heading, before everything else.
    const details = screen.getByRole("heading", { name: "Caskade" });
    expect(follows(heading, credits) && follows(credits, details)).toBe(true);

    expect(screen.getByText(appConfig.version)).toBeTruthy();
    expect(screen.getByText("Browser preview")).toBeTruthy();
    expect(screen.getByRole("link", { name: "Project page" }).getAttribute("href")).toBe(
      "https://github.com/krazyjakee/caskade",
    );
    for (const { label } of legalSections)
      expect(screen.getByRole("link", { name: label.message }));
  },
);

it("opens each Legal section from About, links between them, and comes back", async () => {
  const app = renderApp("/settings?tab=about");
  fireEvent.click(await screen.findByRole("link", { name: "Licenses" }));
  await waitFor(() =>
    expect(app.router.state.location.search).toEqual({ tab: "about", section: "licenses" }),
  );
  const nav = await screen.findByRole("navigation", { name: "Legal sections" });
  expect(within(nav).getByRole("link", { name: "Licenses" }).getAttribute("aria-current")).toBe(
    "page",
  );
  const licences = await screen.findByRole("article", { name: "Licenses" });
  // Focusable, so keys and a pad scroll it.
  expect(licences.tabIndex).toBe(0);
  expect(within(licences).getByText(/released under the MIT License/)).toBeTruthy();
  for (const { platforms } of notices.sevenZip.licenses)
    expect(within(licences).getByRole("heading", { name: new RegExp(platforms.join(", ")) }));

  for (const { id, label: descriptor } of legalSections.filter(({ id }) => id !== "licenses")) {
    const label = descriptor.message as string;
    fireEvent.click(within(nav).getByRole("link", { name: label }));
    await waitFor(() =>
      expect(app.router.state.location.search).toEqual({ tab: "about", section: id }),
    );
    expect(await screen.findByRole("heading", { name: label })).toBeTruthy();
    expect(within(nav).getByRole("link", { name: label }).getAttribute("aria-current")).toBe(
      "page",
    );
  }

  fireEvent.click(screen.getByRole("link", { name: "Back to About" }));
  await waitFor(() => expect(app.router.state.location.search).toEqual({ tab: "about" }));
  expect(await screen.findByRole("list", { name: "Credits" })).toBeTruthy();
});

it("opens a section from its own address, and B goes back to About", async () => {
  const app = renderApp("/settings?tab=about&section=privacy", { mode: "console" });
  const policy = await screen.findByRole("article", { name: "Privacy policy" });
  expect(within(policy).getByRole("heading", { name: "Privacy policy" })).toBeTruthy();
  expect(within(policy).getByText(/no telemetry/)).toBeTruthy();
  expect(within(policy).getByText(/Effective:/)).toBeTruthy();
  app.pad.press("back");
  await waitFor(() => expect(app.router.state.location.search).toEqual({ tab: "about" }));
  expect(await screen.findByRole("list", { name: "Credits" })).toBeTruthy();
});

it("lists the shipped packages by licence, with their holders and the licence text", async () => {
  renderApp("/settings?tab=about&section=packages");
  await screen.findByRole("heading", { name: "Open-source packages" });
  const fonts = screen.getByRole("button", { name: /^OFL-1\.1/ });
  expect(fonts.getAttribute("aria-expanded")).toBe("false");
  fireEvent.click(fonts);
  expect(fonts.getAttribute("aria-expanded")).toBe("true");
  const body = screen.getByRole("article", { name: "Packages under OFL-1.1" });
  for (const font of ["inter", "jetbrains-mono", "oxanium"])
    expect(within(body).getByText(new RegExp(`^@fontsource-variable/${font} `))).toBeTruthy();
  expect(within(body).getAllByText(/SIL Open Font License/).length).toBeGreaterThan(0);
});

it("bundles the inventory: Rust crates, JavaScript packages and 7-Zip for every platform", () => {
  const names = new Set(notices.groups.flatMap((group) => group.packages.map((p) => p.name)));
  for (const name of ["tauri", "rusqlite", "react", "@tanstack/react-router"])
    expect(names.has(name), name).toBe(true);
  expect(notices.sevenZip.licenses.flatMap((licence) => licence.platforms).sort()).toEqual([
    "Linux",
    "Windows",
    "macOS",
  ]);
  for (const group of notices.groups)
    expect(group.packages.length, group.license).toBeGreaterThan(0);
});

it("keeps About for anything that isn't a Legal section", () => {
  expect(legalSection("terms")).toBe("terms");
  expect(legalSection("nope")).toBeUndefined();
  expect(legalSection(undefined)).toBeUndefined();
});

it("reads the Markdown the legal documents use", () => {
  expect(
    parseMarkdown(
      "# Title\n\nOne\nparagraph.\n\n- a\n  b\n- c\n\n> **Missing:** x\n\n```sh\nrun\n```",
    ),
  ).toEqual([
    { kind: "heading", level: 1, text: "Title" },
    { kind: "paragraph", text: "One paragraph." },
    { kind: "list", items: ["a b", "c"] },
    { kind: "quote", text: "**Missing:** x" },
    { kind: "code", text: "run" },
  ]);
});
