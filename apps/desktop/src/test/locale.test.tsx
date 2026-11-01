import { loadCatalog, pseudoLocale } from "@caskade/i18n";
import { cleanup, screen, within } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { renderApp } from "./harness";
import { messages } from "./shell.sv.po";

afterEach(cleanup);

const sidebar = () => within(document.querySelector('[data-cs="sidebar"]') as HTMLElement);

it("shows the shell's marked strings from a PO file, and English for missing and fuzzy entries", async () => {
  renderApp("/", { locale: "sv", messages });
  expect(await screen.findByRole("link", { name: "Bibliotek" })).toBeTruthy();
  expect(sidebar().getByRole("link", { name: "Hem" })).toBeTruthy();
  expect(sidebar().getByRole("link", { name: "Inställningar" })).toBeTruthy();
  // "Store" is fuzzy and "Collections" missing: both show English.
  expect(sidebar().getByRole("link", { name: "Store" })).toBeTruthy();
  expect(sidebar().getByRole("link", { name: "Collections" })).toBeTruthy();
});

it("shows English by default", async () => {
  renderApp("/");
  expect(await screen.findByRole("link", { name: "Library" })).toBeTruthy();
  expect(sidebar().getByRole("link", { name: "Home" })).toBeTruthy();
});

it("renders the pseudo-locale from the templates", async () => {
  const locale = pseudoLocale ?? "en-XA";
  renderApp("/", { locale, messages: await loadCatalog(locale) });
  const library = await screen.findByRole("link", { name: /^\[.*Ĺĩƀŕàŕŷ.*\]$/ });
  expect(library.textContent).not.toContain("Library");
});
