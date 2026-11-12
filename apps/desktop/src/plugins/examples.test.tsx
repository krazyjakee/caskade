// #133, #109: the example plugins' UI in the webview. From plugin API 0.2
// their backends draw it (plugins/examples/*/backend); here a fake host
// answers with the trees they draw, and the loader registers them through
// the drawn path. What the backends really draw is tested in
// crates/caskade-plugin-host/tests/examples.rs; the native app's drawing in
// apps/native/src/plugins/tests.rs.
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import type { Node_Serialize as UiNode } from "../api/bindings";
import { fakeLayout, fakeViewport, renderApp } from "../test/harness";
import { PluginGameDetails } from "./GameSlots";
import { PluginLoader } from "./loader";
import { PluginSlot } from "./PluginSlot";
import { PluginRegistry, PluginRegistryContext } from "./registry";
import type { PluginUiApi, UiPlugin } from "./types";

afterEach(cleanup);

const drawn = (id: string, name: string, contributes: UiPlugin["contributes"]): UiPlugin => ({
  id,
  name,
  version: "0.2.0",
  entry: "",
  drawn: true,
  contributes,
});

const stats = drawn("dev.caskade.playtime-stats", "Playtime stats", [
  { slot: "views", id: "stats", title: "Playtime stats", modes: ["desktop", "console"] },
]);

/** Playtime stats' view, as its backend draws it. */
function playtime(view: string): UiNode[] {
  return [
    {
      type: "section",
      label: "Most played",
      children: [
        { type: "tile", value: "53 h", label: "across your 2 most played games" },
        {
          type: "segmented",
          label: "Show as",
          action: "view",
          value: view,
          options: [
            { value: "chart", label: "Chart" },
            { value: "table", label: "Table" },
          ],
        },
        view === "table"
          ? {
              type: "table",
              caption: "Most played games",
              columns: ["Game", "Hours played"],
              rows: [
                ["Celeste", "40.0"],
                ["Hades", "12.5"],
              ],
              rowHeaders: true,
            }
          : {
              type: "bars",
              label: "Most played games, hours played",
              items: [
                { label: "Celeste", value: 40, text: "40 h" },
                { label: "Hades", value: 12.5, text: "13 h" },
              ],
            },
      ],
    },
  ];
}

interface Call {
  kind: "render" | "act";
  plugin: string;
  slot: string;
  id: string;
  action?: string;
  value?: unknown;
  gameId?: string | null;
}

/** A host with `plugins`, drawing with `draw` and answering `act` with
 * `answer`; every call is recorded. */
function host(
  plugins: UiPlugin[],
  draw: (call: Call, state: unknown) => UiNode[] | Promise<UiNode[]>,
  answer: (call: Call) => { state?: unknown; notice?: string } = (call) => ({
    state: { view: call.value },
  }),
) {
  const calls: Call[] = [];
  const api: PluginUiApi = {
    list: async () => plugins,
    installed: async () => [],
    request: async () => null,
    subscribe: async () => () => {},
    settings: {
      get: async () => ({ revision: 0, value: {} }),
      set: async (_id, value, revision) => ({ revision: revision + 1, value }),
    },
    render: async (plugin, slot, id, context) => {
      const call: Call = { kind: "render", plugin, slot, id, gameId: context.gameId ?? null };
      calls.push(call);
      return draw(call, context.state);
    },
    act: async (plugin, slot, id, action, value, context) => {
      const call: Call = { kind: "act", plugin, slot, id, action, value, gameId: context.gameId };
      calls.push(call);
      return answer(call) as Awaited<ReturnType<NonNullable<PluginUiApi["act"]>>>;
    },
  };
  return { api, calls };
}

async function loaded(api: PluginUiApi) {
  const registry = new PluginRegistry();
  await new PluginLoader(api, registry).loadAll();
  expect(registry.listNotices()).toEqual([]);
  return registry;
}

function show(
  registry: PluginRegistry,
  slots: readonly ("views" | "home.sections" | "game.details.panels" | "game.details.tabs")[],
) {
  render(
    <PluginRegistryContext.Provider value={registry}>
      {slots.map((slot) => (
        <PluginSlot key={slot} slot={slot}>
          {({ contribution }) => {
            const Component = (contribution as { component: (props: { gameId: string }) => null })
              .component;
            return <Component gameId="g1" />;
          }}
        </PluginSlot>
      ))}
    </PluginRegistryContext.Provider>,
  );
}

it("a drawn view charts, and its segmented control brings the table (playtime-stats)", async () => {
  const { api, calls } = host([stats], (_call, state) =>
    playtime((state as { view?: string } | null)?.view ?? "chart"),
  );
  show(await loaded(api), ["views"]);
  const bars = await screen.findByRole("list", { name: "Most played games, hours played" });
  const rows = within(bars).getAllByRole("listitem");
  expect(rows.map((row) => row.textContent)).toEqual(["Celeste40 h", "Hades13 h"]);
  expect(screen.getByText("53 h")).toBeTruthy();
  fireEvent.click(screen.getByRole("radio", { name: "Table" }));
  await screen.findByRole("table", { name: "Most played games" });
  expect(calls.filter((call) => call.kind === "act")).toEqual([
    expect.objectContaining({ slot: "views", id: "stats", action: "view", value: "table" }),
  ]);
});

it("a drawn view works in console mode by pad alone (#152)", async () => {
  const { api } = host([stats], (_call, state) =>
    playtime((state as { view?: string } | null)?.view ?? "chart"),
  );
  const registry = await loaded(api);
  const undoLayout = fakeLayout();
  fakeViewport();
  try {
    const app = renderApp("/", { mode: "console", registry });
    // In the console sidebar; confirm opens it.
    const nav = await screen.findByRole("navigation", { name: "Plugins" });
    const link = within(nav).getByRole("link", { name: "Playtime stats" });
    act(() => link.focus());
    app.pad.press("confirm");
    await screen.findByRole("list", { name: "Most played games, hours played" });
    // LB/RB move its segmented control.
    app.pad.press("tab.next");
    await screen.findByRole("table", { name: "Most played games" });
    app.pad.press("tab.prev");
    await screen.findByRole("list", { name: "Most played games, hours played" });
  } finally {
    undoLayout();
  }
});

it("a drawn game tab is drawn for its game (howlongtobeat)", async () => {
  const hltb = drawn("dev.caskade.howlongtobeat", "HowLongToBeat", [
    { slot: "game.details.tabs", id: "hltb", title: "How long to beat" },
  ]);
  const { api, calls } = host([hltb], () => [
    {
      type: "stats",
      items: [
        { label: "Main story", value: "8 h" },
        { label: "Completionist", value: "41 h" },
      ],
    },
    { type: "note", text: "From howlongtobeat.com: Celeste" },
  ]);
  show(await loaded(api), ["game.details.tabs"]);
  await screen.findByText("From howlongtobeat.com: Celeste");
  expect(screen.getByText("Main story").nextElementSibling?.textContent).toBe("8 h");
  expect(calls[0]).toEqual(
    expect.objectContaining({ slot: "game.details.tabs", id: "hltb", gameId: "g1" }),
  );
});

it("a plugin that fails to draw shows placeholders while the others keep drawing (crashy)", async () => {
  const crashy = drawn("dev.caskade.crashy", "Crashy", [
    { slot: "views", id: "broken", title: "Crashy" },
    { slot: "home.sections", id: "broken", title: "Crashy" },
    { slot: "game.details.panels", id: "broken" },
  ]);
  const { api } = host([crashy, stats], (call) => {
    if (call.plugin === crashy.id) throw new Error("crashy failed to draw");
    return playtime("chart");
  });
  const registry = await loaded(api);
  const errors = console.error;
  console.error = () => {};
  try {
    show(registry, ["views", "home.sections", "game.details.panels"]);
    await screen.findByRole("list", { name: "Most played games, hours played" });
    await waitFor(() =>
      expect(
        screen
          .getAllByRole("alert")
          .filter((element) => element.dataset.plugin === "dev.caskade.crashy"),
      ).toHaveLength(3),
    );
    const placeholder = screen
      .getAllByRole("alert")
      .find((element) => element.dataset.plugin === "dev.caskade.crashy");
    expect(placeholder?.textContent).toBe("Crashy couldn't show this part.");
  } finally {
    console.error = errors;
  }
});

it("a drawn game action runs for its game and its notice shows (caskade-pack)", async () => {
  const pack = drawn("dev.caskade.example.pack", "Caskade Pack (example)", [
    { slot: "game.actions", id: "about-packs", title: "About game packs" },
  ]);
  const { api, calls } = host(
    [pack],
    () => [],
    () => ({ notice: "Caskade installs .cpk game packs." }),
  );
  const registry = await loaded(api);
  render(
    <PluginRegistryContext.Provider value={registry}>
      <PluginGameDetails gameId="g1" />
    </PluginRegistryContext.Provider>,
  );
  fireEvent.click(screen.getByRole("button", { name: "About game packs" }));
  await waitFor(() =>
    expect(registry.listNotices().at(-1)?.message).toBe("Caskade installs .cpk game packs."),
  );
  expect(calls).toEqual([
    expect.objectContaining({ kind: "act", action: "run", value: null, gameId: "g1" }),
  ]);
});
