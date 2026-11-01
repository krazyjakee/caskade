import { activateLocale, sourceLocale } from "@caskade/i18n";
import { act, cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createEmulationFixtureApi } from "../api/emulationFixtures";
import { formatSource } from "../api/presentation";
import { fakeLayout } from "../test/harness";
import { discovery, renderOnboarding, romm } from "../test/onboardingFixture";
import { createSettingsFixture } from "../test/settingsFixture";
import { hideBootSplash } from "./boot";
import { defaultChoices, runSetup, type Task } from "./run";

afterEach(cleanup);

describe("the setup run", () => {
  it("saves the choices, then finds games, emulators, ROMs and RomM's library", async () => {
    const settings = createSettingsFixture();
    const found = discovery();
    const romms = romm();
    const cues: string[] = [];
    let last: Task[] = [];
    const tasks = await runSetup(
      {
        settings: settings.api,
        discovery: found.api,
        emulation: createEmulationFixtureApi(),
        romm: romms,
      },
      {
        ...defaultChoices,
        playTime: false,
        romFolder: "/home/player/ROMs",
        romm: { url: "romm.lan:8080", auth: "password", username: " player ", secret: "secret" },
      },
      {
        update: (next) => {
          last = next;
        },
        cue: (cue) => cues.push(cue),
        pollMs: 1,
      },
    );

    expect(tasks.map((task) => [task.id, task.state])).toEqual([
      ["games", "done"],
      ["emulators", "done"],
      ["roms", "done"],
      ["romm", "done"],
      ["artwork", "done"],
    ]);
    expect(last).toEqual(tasks);
    expect(tasks[0]?.detail).toBe("12 games");
    expect(tasks[1]?.detail).toBe("RetroArch · PCSX2");
    expect(tasks[3]?.detail).toBe("32 games");
    expect(romms.saved).toEqual(["romm-lan https://romm.lan:8080 password:player:secret"]);
    expect(settings.values["discovery.import"]).toEqual({ seedPlaytime: false });
    expect(settings.values["core.onboarding"]?.completed).toBe(true);
    // The scan starts before setup counts as done, so the core doesn't
    // start a second one.
    expect(settings.writes.indexOf("core.onboarding")).toBeGreaterThan(
      settings.writes.indexOf("core.discovery"),
    );
    expect(found.scan).toHaveBeenCalledOnce();
    expect(cues).toContain("found");
  });

  it("signs in to RomM with an API token", async () => {
    const romms = romm();
    const tasks = await runSetup(
      {
        settings: createSettingsFixture().api,
        discovery: discovery().api,
        emulation: createEmulationFixtureApi(),
        romm: romms,
      },
      {
        ...defaultChoices,
        pcGames: false,
        online: false,
        romm: {
          url: "https://romm.lan",
          auth: "token",
          username: "left over",
          secret: " rmm_abc ",
        },
      },
      { update: () => {}, cue: () => {}, pollMs: 1 },
    );
    expect(tasks.map((task) => task.state)).toEqual(["done"]);
    expect(romms.saved).toEqual(["romm-lan https://romm.lan token::rmm_abc"]);
  });

  it("turns off what was turned off", async () => {
    const settings = createSettingsFixture();
    const found = discovery();
    const tasks = await runSetup(
      {
        settings: settings.api,
        discovery: found.api,
        emulation: createEmulationFixtureApi(),
        romm: romm(),
      },
      { ...defaultChoices, pcGames: false, online: false },
      { update: () => {}, cue: () => {}, pollMs: 1 },
    );
    expect(tasks).toEqual([]);
    expect(found.scan).not.toHaveBeenCalled();
    expect(settings.values["core.discovery"]?.enabled).toBe(false);
    expect(settings.values["core.metadata"]).toEqual({ online: false });
    expect(settings.values["core.onboarding"]?.completed).toBe(true);
  });

  it("names sources in the user's language", async () => {
    await activateLocale("sv");
    try {
      expect(["steam", "steam.shortcuts", "desktop"].map(formatSource)).toEqual([
        "Steam",
        "Steam-genvägar",
        "Skrivbordsposter",
      ]);
    } finally {
      await activateLocale(sourceLocale);
    }
  });

  it("counts the games added, and warns when those found all wait for review", async () => {
    const run = async (counts: { found: number; imported: number }) =>
      (
        await runSetup(
          {
            settings: createSettingsFixture().api,
            discovery: discovery(counts).api,
            emulation: createEmulationFixtureApi(),
            romm: romm(),
          },
          { ...defaultChoices, online: false },
          { update: () => {}, cue: () => {}, pollMs: 1 },
        )
      )[0];
    // Three of the ten wait for review.
    expect(await run({ found: 10, imported: 7 })).toMatchObject({
      state: "done",
      detail: "7 games",
    });
    expect(await run({ found: 3, imported: 0 })).toMatchObject({
      state: "warning",
      detail: "3 games",
    });
  });
});

describe("first-run setup", () => {
  it("walks from the welcome to the library", async () => {
    const { settings, onDone, consoleApi, library } = renderOnboarding();

    fireEvent.click(await screen.findByRole("button", { name: "Get started" }));
    // The language first: System, English here.
    expect(await screen.findByRole("radio", { name: "System (English)" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    expect(await screen.findByText("/home/player/Games/Caskade")).toBeTruthy();
    expect(screen.getByText("400 GiB free of 1.0 TiB")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));

    fireEvent.click(await screen.findByRole("switch", { name: "Artwork and game info" }));
    fireEvent.click(screen.getByRole("switch", { name: "Retro games" }));
    // A ROMs folder is needed before going on.
    expect(screen.getByRole("button", { name: "Continue" }).hasAttribute("disabled")).toBe(true);
    fireEvent.click(screen.getByRole("switch", { name: "Retro games" }));
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));

    fireEvent.click(await screen.findByRole("radio", { name: /Console mode/ }));
    expect(consoleApi.calls).toEqual(["console"]);
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));

    // The admin's account: a name, an avatar, and an optional PIN.
    const name = await screen.findByRole("textbox", { name: "Name" });
    fireEvent.change(name, { target: { value: "Ada" } });
    fireEvent.click(screen.getByRole("radio", { name: "Rocket" }));
    const pin = screen.getByLabelText("PIN");
    fireEvent.change(pin, { target: { value: "12" } });
    expect(screen.getByRole("button", { name: "Set up" }).hasAttribute("disabled")).toBe(true);
    fireEvent.change(pin, { target: { value: "1234" } });
    fireEvent.click(screen.getByRole("button", { name: "Set up" }));

    const open = await screen.findByRole("button", { name: "Open my library" }, { timeout: 5000 });
    const games = document.querySelector('[data-task="games"]');
    expect(games?.getAttribute("data-status")).toBe("done");
    expect(games?.textContent).toContain("12 games");
    expect(document.querySelector('[data-task="artwork"]')).toBeNull();
    expect(settings.values["core.metadata"]).toEqual({ online: false });
    expect(settings.values["core.onboarding"]?.completed).toBe(true);
    const [admin] = (await library.accounts?.list())?.accounts ?? [];
    expect(admin).toMatchObject({ name: "Ada", avatar: "avatar:rocket", hasPin: true });

    fireEvent.click(open);
    expect(onDone).toHaveBeenCalledOnce();
    expect(screen.getByText("The library")).toBeTruthy();
  });

  it("plays the startup sound as the logo shows, before any press", async () => {
    renderOnboarding();
    await screen.findByRole("button", { name: "Get started" });
    expect(HTMLMediaElement.prototype.play).toHaveBeenCalledOnce();
  });

  it("skips setup and keeps the defaults", async () => {
    const found = discovery();
    const { settings, onDone } = renderOnboarding(createSettingsFixture(), found);
    fireEvent.click(await screen.findByRole("button", { name: "Skip setup" }));
    await screen.findByText("The library");
    expect(onDone).toHaveBeenCalledOnce();
    expect(settings.values["core.onboarding"]?.completed).toBe(true);
    // The core's own first scan runs once setup is done.
    expect(found.scan).not.toHaveBeenCalled();
  });

  it("shows the app when setup is done, and setup again when asked", async () => {
    const settings = createSettingsFixture();
    settings.values["core.onboarding"] = { completed: true, sounds: true };
    renderOnboarding(settings);
    await screen.findByText("The library");
    expect(screen.queryByRole("button", { name: "Get started" })).toBeNull();

    // Settings → General → Run setup again.
    await act(async () => {
      const current = await settings.api.get("core.onboarding");
      await settings.api.set(
        "core.onboarding",
        { completed: false, sounds: true },
        current.revision,
      );
    });
    expect(await screen.findByRole("button", { name: "Get started" })).toBeTruthy();
  });

  it("starts again from the current settings", async () => {
    const settings = createSettingsFixture();
    settings.values["core.metadata"] = { online: false };
    renderOnboarding(settings);
    fireEvent.click(await screen.findByRole("button", { name: "Get started" }));
    fireEvent.click(await screen.findByRole("button", { name: "Continue" }));
    await screen.findByText("/home/player/Games/Caskade");
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    const artwork = await screen.findByRole("switch", { name: "Artwork and game info" });
    expect((artwork as HTMLInputElement).checked).toBe(false);
  });

  it("asks RomM for a token instead of a username and password", async () => {
    renderOnboarding();
    fireEvent.click(await screen.findByRole("button", { name: "Get started" }));
    fireEvent.click(await screen.findByRole("button", { name: "Continue" }));
    await screen.findByText("/home/player/Games/Caskade");
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    fireEvent.click(await screen.findByRole("switch", { name: "RomM" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Address" }), {
      target: { value: "https://romm.lan" },
    });
    expect(screen.getByLabelText("Username")).toBeTruthy();
    fireEvent.click(screen.getByRole("radio", { name: "API token" }));
    expect(screen.queryByLabelText("Username")).toBeNull();
    const next = screen.getByRole("button", { name: "Continue" });
    expect(next.hasAttribute("disabled")).toBe(true);
    fireEvent.change(screen.getByLabelText("API token"), { target: { value: "rmm_abc" } });
    expect(next.hasAttribute("disabled")).toBe(false);
  });

  it("starts with the language, preselected from the OS, and the keyboard picks another", async () => {
    const { settings } = renderOnboarding(createSettingsFixture(), discovery(), ["sv-SE"]);
    fireEvent.click(await screen.findByRole("button", { name: "Get started" }));
    const system = (await screen.findByRole("radio", {
      name: "System (Svenska)",
    })) as HTMLInputElement;
    expect(system.checked).toBe(true);
    // The choices wait for the saved setting.
    await waitFor(() => expect(system.disabled).toBe(false));
    // Arrows pick the next language, and save it.
    fireEvent.keyDown(system, { key: "ArrowDown" });
    await waitFor(() => expect(settings.values["core.general"]?.language).toBe("en"));
  });

  it("picks the language with a pad, and setup switches to it at once", async () => {
    const undoLayout = fakeLayout();
    try {
      const { settings, pad } = renderOnboarding();
      fireEvent.click(await screen.findByRole("button", { name: "Get started" }));
      await screen.findByRole("radio", { name: "System (English)" });
      // A pad starts on Continue; up reaches the last language, A picks it.
      const next = screen.getByRole("button", { name: "Continue" });
      pad.press("nav.up");
      await waitFor(() => expect(document.activeElement).toBe(next));
      pad.press("nav.up");
      const last = screen.getAllByRole("radio").at(-1) as HTMLInputElement;
      expect(document.activeElement).toBe(last);
      pad.press("confirm");
      await waitFor(() => expect(settings.values["core.general"]?.language).toBe(last.value));
      await waitFor(() =>
        expect(screen.queryByRole("heading", { name: "Choose your language" })).toBeNull(),
      );
    } finally {
      undoLayout();
      await act(() => activateLocale(sourceLocale));
    }
  });

  it("turns its sounds off and remembers it", async () => {
    const { settings } = renderOnboarding();
    fireEvent.click(await screen.findByRole("button", { name: "Turn sounds off" }));
    expect(screen.getByRole("button", { name: "Turn sounds on" })).toBeTruthy();
    await waitFor(() => expect(settings.values["core.onboarding"]?.sounds).toBe(false));
  });
});

describe("the boot splash", () => {
  it("fades out and goes", () => {
    vi.useFakeTimers();
    try {
      const splash = document.createElement("div");
      splash.id = "boot";
      document.body.append(splash);
      hideBootSplash();
      expect(splash.dataset.state).toBe("done");
      vi.advanceTimersByTime(1000);
      expect(document.getElementById("boot")).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });
});
