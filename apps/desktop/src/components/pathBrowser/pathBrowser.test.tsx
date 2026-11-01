/** Console mode's in-app file and folder picker, driven by a pad: every
 * picker goes through `usePathPicker`, which shows the browser in console
 * mode and the native dialog on the desktop. */
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { BrowseApiContext } from "../../api/browse";
import { createBrowseFixtureApi } from "../../api/browseFixtures";
import { InputProvider } from "../../input";
import { type Mode, ModeProvider } from "../../shell/console/mode";
import {
  fakeConsoleApi,
  fakeLayout,
  fakePad,
  fakeViewport,
  focused,
  renderApp,
  until,
} from "../../test/harness";
import type { PathRequest } from "./FolderBrowser";
import { PathPickerProvider, usePathPicker } from "./PathPicker";

let undoLayout = () => {};
beforeEach(() => {
  undoLayout = fakeLayout();
  fakeViewport();
});
afterEach(() => {
  undoLayout();
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

const tree = createBrowseFixtureApi();
const withBrowser = (app: ReactNode) => <BrowseApiContext value={tree}>{app}</BrowseApiContext>;
const on = (name: string) => until(() => focused()?.textContent === name, name);
const location = () =>
  document.querySelector('[data-cs="folder-browser"] [data-part="location"]')?.textContent;

it("Settings → Storage's Browse… opens the browser; a pad walks the folders and picks one", async () => {
  const app = renderApp("/settings?tab=storage", { mode: "console", wrap: withBrowser });
  // A tap opens it (Steam Deck's touchscreen).
  const browse = await screen.findByRole("button", { name: "Browse…" });
  // The view has placed its first focus.
  await until(() => focused() !== document.body, "the view's first stop");
  fireEvent.click(browse);
  await screen.findByRole("dialog", { name: "Add a storage location" });
  // Focus starts on the first entry, not on the dialog.
  await on("Downloads");
  expect(location()).toBe("/home/player");
  // The browser's buttons are in the hint bar.
  for (const hint of ["Previous place", "Next place", "Page up", "Page down", "Select folder"])
    await screen.findByText(hint);

  // Into Games: focus lands on its first entry.
  app.pad.press("nav.down");
  await on("Emulators");
  app.pad.press("nav.down");
  await on("Games");
  app.pad.press("confirm");
  await on("Caskade");
  expect(location()).toBe("/home/player/Games");

  // Up one folder, back onto the folder just left.
  app.pad.press("nav.up");
  await on("Up one folder");
  app.pad.press("confirm");
  await on("Games");
  expect(location()).toBe("/home/player");

  // RB and LB step through the places.
  app.pad.press("tab.next");
  await until(() => location() === "/", "the file system");
  await on("home");
  app.pad.press("tab.next");
  await until(() => location() === "/run/media/player/SD Card", "the SD card");
  expect(document.querySelector('[data-part="place"][data-status="active"]')?.textContent).toBe(
    "SD Card",
  );
  app.pad.press("tab.prev");
  await until(() => location() === "/", "the file system again");
  app.pad.press("tab.prev");
  await until(() => location() === "/home/player", "home again");

  // Start picks the folder shown.
  app.pad.press("nav.down");
  await on("Emulators");
  app.pad.press("confirm");
  await until(() => location() === "/home/player/Emulators", "Emulators");
  app.pad.press("menu");
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(screen.getByRole("textbox", { name: "Folder" })).toHaveProperty(
    "value",
    "/home/player/Emulators",
  );
});

it("B cancels and closes the browser", async () => {
  const app = renderApp("/settings?tab=storage", { mode: "console", wrap: withBrowser });
  const browse = await screen.findByRole("button", { name: "Browse…" });
  // The view has placed its first focus.
  await until(() => focused() !== document.body, "the view's first stop");
  fireEvent.click(browse);
  await screen.findByRole("dialog", { name: "Add a storage location" });
  await on("Downloads");
  app.pad.press("back");
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(screen.getByRole("textbox", { name: "Folder" })).toHaveProperty("value", "");
  // B with nothing open goes back as usual.
  expect(app.router.state.location.pathname).toBe("/settings");
});

function Pick({
  request,
  native,
  picked,
}: {
  request: PathRequest;
  native: () => Promise<string | null>;
  picked: (path: string | null) => void;
}) {
  const picker = usePathPicker();
  return (
    <button type="button" onClick={() => void picker.pick(request, native).then(picked)}>
      Pick
    </button>
  );
}

/** One picker on its own, under a pad. */
function renderPicker(mode: Mode, request: PathRequest, native = vi.fn(async () => "/native")) {
  const pad = fakePad();
  const picked = vi.fn();
  render(
    <ModeProvider api={fakeConsoleApi().api} initial={mode}>
      <BrowseApiContext value={tree}>
        <InputProvider console={mode === "console"} subscribe={pad.subscribe}>
          <PathPickerProvider>
            <Pick request={request} native={native} picked={picked} />
          </PathPickerProvider>
        </InputProvider>
      </BrowseApiContext>
    </ModeProvider>,
  );
  return { pad, picked, native, open: () => fireEvent.click(screen.getByText("Pick")) };
}

it("LT/RT page through a long folder; A picks a file of the kinds asked for", async () => {
  const { pad, picked, native, open } = renderPicker("console", {
    kind: "file",
    title: "Choose a ROM",
    extensions: ["sfc"],
    startAt: "/home/player/ROMs/snes",
  });
  open();
  await screen.findByRole("dialog", { name: "Choose a ROM" });
  await on("Chrono Trigger.sfc");
  const rows = () =>
    Array.from(document.querySelectorAll<HTMLElement>('[data-part="entries"] button'));
  const index = () => rows().indexOf(focused() as HTMLElement);
  const first = index();
  pad.press("page.next");
  await until(() => index() > first + 5, "a page down");
  const paged = index();
  pad.press("page.next");
  await until(() => index() > paged, "another page down");
  pad.press("page.prev");
  await until(() => index() === paged, "a page up");
  pad.press("page.prev");
  pad.press("page.prev");
  await until(() => index() === 0, "the top");
  await on("Up one folder");

  // A file is picked with A.
  pad.press("nav.down");
  await on("Chrono Trigger.sfc");
  pad.press("confirm");
  await waitFor(() =>
    expect(picked).toHaveBeenCalledWith("/home/player/ROMs/snes/Chrono Trigger.sfc"),
  );
  expect(native).not.toHaveBeenCalled();
  expect(screen.queryByRole("dialog")).toBeNull();
});

it("files of other kinds can't be picked", async () => {
  const { open } = renderPicker("console", {
    kind: "file",
    title: "Install from file",
    extensions: ["zip"],
    startAt: "/home/player/Downloads",
  });
  open();
  await on("starfield-patch.zip");
  expect(screen.getByRole("button", { name: "setup-quest.exe" })).toHaveProperty("disabled", true);
});

it("a start folder that has gone opens home", async () => {
  const { open } = renderPicker("console", {
    kind: "folder",
    title: "Choose a folder",
    startAt: "/home/player/Gone",
  });
  open();
  await on("Downloads");
  expect(location()).toBe("/home/player");
});

it("desktop mode keeps the native picker", async () => {
  const { picked, native, open } = renderPicker("desktop", { kind: "folder", title: "Choose" });
  await act(async () => open());
  await waitFor(() => expect(picked).toHaveBeenCalledWith("/native"));
  expect(native).toHaveBeenCalledTimes(1);
  expect(screen.queryByRole("dialog")).toBeNull();
});
