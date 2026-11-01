import { pseudoLocale, sourceLocale } from "@caskade/i18n";
import { act, cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { createSettingsLocaleApi, negotiate } from "../api/locale";
import { renderApp } from "../test/harness";
import { createSettingsFixture } from "../test/settingsFixture";
import { languageName } from "./LocaleProvider";

afterEach(cleanup);

const pseudo = pseudoLocale ?? "en-XA";
// The sidebar's Library link in the pseudo-locale: accented and bracketed.
const pseudoLibrary = { name: /^\[.*Ĺĩƀŕàŕŷ.*\]$/ };

describe("resolving the Language setting", () => {
  it("falls back through a tag's less specific forms, then English", () => {
    const available = ["en", "pt", "sv"];
    expect(negotiate(["sv-SE"], available)).toBe("sv");
    expect(negotiate(["pt_BR.UTF-8"], available)).toBe("pt");
    expect(negotiate(["de-DE", "sv"], available)).toBe("sv");
    expect(negotiate(["en-GB", "sv"], available)).toBe("en");
    // An OS language Caskade doesn't ship is English.
    expect(negotiate(["de-DE"], available)).toBe("en");
    expect(negotiate([], available)).toBe("en");
  });

  it("follows the setting, and the OS's language on System", async () => {
    const settings = createSettingsFixture();
    const api = createSettingsLocaleApi({
      settings: settings.api,
      system: () => ["de-DE"],
      available: ["en", "sv"],
    });
    expect(await api.current()).toEqual({ locale: "en", system: "en", available: ["en", "sv"] });
    const seen: string[] = [];
    await api.subscribe((locale) => seen.push(locale));
    const current = await settings.api.get("core.general");
    await settings.api.set("core.general", { ...current.value, language: "sv" }, current.revision);
    await waitFor(() => expect(seen).toEqual(["sv"]));
    expect((await api.current()).locale).toBe("sv");
  });

  it("names each language in itself", () => {
    expect(languageName("sv")).toBe("Svenska");
    expect(languageName("de")).toBe("Deutsch");
    expect(languageName(sourceLocale)).toBe("English");
  });
});

/** Settings → General → Language's list. */
async function languageSelect() {
  const buttons = await screen.findAllByRole("button", { name: /Language/ });
  const trigger = buttons.find((button) => button.getAttribute("aria-haspopup") === "listbox");
  if (!trigger) throw new Error("no Language list");
  return trigger;
}

/** Opens the Language list and picks `option`. */
async function pickLanguage(option: string | RegExp) {
  fireEvent.click(await languageSelect());
  fireEvent.click(await screen.findByRole("option", { name: option }));
}

describe("switching the language", () => {
  it("shows System as what it resolves to, then each language by its own name", async () => {
    renderApp("/settings?tab=general", { systemLanguages: ["sv-SE"] });
    fireEvent.click(await languageSelect());
    expect(await screen.findByRole("option", { name: "System (Svenska)" })).toBeTruthy();
    expect(screen.getByRole("option", { name: "English" })).toBeTruthy();
    expect(screen.getByRole("option", { name: /^Svenska/ })).toBeTruthy();
  });

  it("switches the desktop shell live, with <html lang>", async () => {
    const app = renderApp("/settings?tab=general");
    expect(await screen.findByRole("link", { name: "Library" })).toBeTruthy();
    await pickLanguage(languageName(pseudo));
    expect(await screen.findByRole("link", pseudoLibrary)).toBeTruthy();
    expect(app.settings.values["core.general"]?.language).toBe(pseudo);
    expect(document.documentElement.lang).toBe(pseudo);

    // Back to System: English, as the OS is.
    await act(async () => {
      const current = await app.settings.api.get("core.general");
      await app.settings.api.set(
        "core.general",
        { ...current.value, language: "system" },
        current.revision,
      );
    });
    expect(await screen.findByRole("link", { name: "Library" })).toBeTruthy();
    expect(document.documentElement.lang).toBe(sourceLocale);
  });

  it("switches the console shell live", async () => {
    const app = renderApp("/settings?tab=general", { mode: "console" });
    const sidebar = () => document.querySelector('[data-cs="console-sidebar"]') as HTMLElement;
    await waitFor(() => expect(sidebar()?.textContent).toContain("Library"));
    await pickLanguage(languageName(pseudo));
    await waitFor(() => expect(sidebar().textContent).not.toContain("Library"));
    expect(await screen.findByRole("link", pseudoLibrary)).toBeTruthy();
    expect(app.settings.values["core.general"]?.language).toBe(pseudo);
  });
});
