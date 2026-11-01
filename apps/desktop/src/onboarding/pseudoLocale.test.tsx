/** First-run setup in the pseudo-locale (#217): every step, from the welcome
 * to the finished setup, shows only catalog text. The OS's language is the
 * pseudo-locale, so System picks it as the core would. */
import { activateLocale, pseudoLocale, sourceLocale } from "@caskade/i18n";
import { act, cleanup, fireEvent, waitFor, within } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { discovery, renderOnboarding } from "../test/onboardingFixture";
import { createSettingsFixture } from "../test/settingsFixture";
import { untranslated } from "../test/untranslated";

vi.setConfig({ testTimeout: 20_000 });

afterEach(async () => {
  cleanup();
  await act(() => activateLocale(sourceLocale));
});

const locale = pseudoLocale ?? "en-XA";

/** Names, paths and brands: data, not interface. */
const data = [
  /CASKADE/g,
  /RomM/g,
  /RetroArch|PCSX2/g,
  // Launchers the scan reports while it runs.
  /Steam|Heroic|Lutris|Epic|GOG|EA app|Battle\.net|itch\.io|Xbox/g,
  /\/home\/player\/[\w/]*/g,
  // The language list names each language in itself.
  /English|Svenska/g,
];

const one = (selector: string) => document.querySelector(selector) as HTMLElement | null;
const shown = () =>
  untranslated([one('[data-cs="onboarding"]')], {
    data,
    // The library's covers behind the setup are games.
    skip: '[data-part="covers"], [data-part="language"]',
  });
const press = (selector: string) => fireEvent.click(one(selector) as HTMLElement);
const step = (name: string) =>
  waitFor(() => expect(one(`[data-cs="onboarding"][data-step="${name}"]`)).not.toBeNull());

it("shows every step of first-run setup in the pseudo-locale", async () => {
  // As main.tsx does, the language System resolves to is active before the
  // first render.
  await act(() => activateLocale(locale));
  renderOnboarding(createSettingsFixture(), discovery(), [locale]);
  // The welcome, once the language is the pseudo-locale.
  await waitFor(() => expect(one('[data-part="start"]')?.textContent).toMatch(/^\[.+\]$/));
  expect(shown()).toEqual([]);

  press('[data-part="start"]');
  await step("language");
  await waitFor(() => expect(shown()).toEqual([]));

  press('[data-part="next"]');
  await step("folder");
  await waitFor(() => expect(one('[data-part="card"] .font-mono')).not.toBeNull());
  expect(shown()).toEqual([]);

  press('[data-part="next"]');
  await step("sources");
  // Retro games and RomM show what they need once they're on.
  const sources = () => [...document.querySelectorAll<HTMLElement>('[data-part="source"]')];
  await waitFor(() => expect(sources()).toHaveLength(4));
  for (const index of [1, 2])
    fireEvent.click(within(sources()[index] as HTMLElement).getByRole("switch"));
  await waitFor(() => expect(sources()[2]?.querySelector("input[type=password]")).not.toBeNull());
  expect(shown()).toEqual([]);
  // The API token's fields.
  fireEvent.click(within(sources()[2] as HTMLElement).getAllByRole("radio")[1] as HTMLElement);
  expect(shown()).toEqual([]);
  for (const index of [1, 2])
    fireEvent.click(within(sources()[index] as HTMLElement).getByRole("switch"));

  press('[data-part="next"]');
  await step("mode");
  expect(shown()).toEqual([]);

  press('[data-part="next"]');
  await step("account");
  expect(shown()).toEqual([]);

  press('[data-part="next"]');
  await step("setup");
  expect(shown()).toEqual([]);
  await waitFor(() => expect(one('[data-part="setup"][data-status="done"]')).not.toBeNull(), {
    timeout: 10_000,
  });
  expect(shown()).toEqual([]);
});
