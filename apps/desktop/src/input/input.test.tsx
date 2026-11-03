import { FocusSection, useFocusable } from "@caskade/sdk";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useRef, useState } from "react";
import {
  Button,
  Checkbox,
  Dialog,
  DialogTrigger,
  Input,
  Label,
  ListBox,
  ListBoxItem,
  Modal,
  Popover,
  Radio,
  RadioGroup,
  Select,
  SelectValue,
  Slider,
  SliderThumb,
  SliderTrack,
  Switch,
  TextField,
} from "react-aria-components";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Action } from "./actions";
import { keyAction } from "./actions";
import { InputProvider, type PadAction, useActiveBindings, useInputAction } from "./index";

// jsdom has no layout: elements get boxes from `data-rect="x y w h"`;
// overlays fill the screen and listbox options stack in order.
const original = Element.prototype.getBoundingClientRect;
const box = (x: number, y: number, w: number, h: number) =>
  ({ x, y, left: x, top: y, width: w, height: h, right: x + w, bottom: y + h }) as DOMRect;
beforeEach(() => {
  Element.prototype.getBoundingClientRect = function (this: Element) {
    const own = this.getAttribute("data-rect");
    if (own) {
      const [x, y, w, h] = own.split(" ").map(Number) as [number, number, number, number];
      return box(x, y, w, h);
    }
    if (this.matches('[role="dialog"], [data-placement], [data-cs="view"]'))
      return box(0, 0, 1600, 900);
    if (this.matches('[role="option"]')) {
      const index = Array.from(this.parentElement?.children ?? []).indexOf(this);
      return box(600, 300 + index * 40, 200, 36);
    }
    return box(0, 0, 0, 0);
  };
});
afterEach(() => {
  Element.prototype.getBoundingClientRect = original;
  cleanup();
});

/** Pad actions come from here instead of the core. */
function padSource() {
  let send: (action: PadAction) => void = () => {};
  return {
    subscribe: async (listener: (action: PadAction) => void) => {
      send = listener;
      return () => {};
    },
    press: (action: Action) =>
      act(() => {
        send({ action, glyphs: "xbox" });
      }),
  };
}

const keys: Record<string, string> = {
  "nav.up": "ArrowUp",
  "nav.down": "ArrowDown",
  "nav.left": "ArrowLeft",
  "nav.right": "ArrowRight",
  confirm: "Enter",
  back: "Escape",
  "tab.next": "]",
};

/** Sidebar on the left; content with a shelf, a grid, a dialog, a select
 * and a text field. */
function Layout() {
  return (
    <>
      <FocusSection id="sidebar">
        {["Home", "Library", "Settings"].map((label, i) => (
          <Button key={label} data-rect={`0 ${i * 50} 200 40`}>
            {label}
          </Button>
        ))}
      </FocusSection>
      <FocusSection id="main">
        <FocusSection id="shelf">
          {["A", "B", "C"].map((label, i) => (
            <Button key={label} data-rect={`${300 + i * 150} 0 140 80`}>
              Shelf {label}
            </Button>
          ))}
        </FocusSection>
        <FocusSection id="grid">
          {[0, 1, 2, 3, 4, 5].map((i) => (
            <Button
              key={i}
              data-rect={`${300 + (i % 3) * 150} ${100 + Math.floor(i / 3) * 100} 140 80`}
            >
              Card {i}
            </Button>
          ))}
        </FocusSection>
        <DialogTrigger>
          <Button data-rect="300 320 140 40">Open dialog</Button>
          <Modal>
            <Dialog aria-label="Confirm">
              <Button data-rect="600 400 100 40">Yes</Button>
              <Button data-rect="720 400 100 40">No</Button>
            </Dialog>
          </Modal>
        </DialogTrigger>
        <Select aria-label="Sort" data-rect="460 320 140 40">
          <Button data-rect="460 320 140 40">
            <SelectValue />
          </Button>
          <Popover>
            <ListBox>
              <ListBoxItem id="title">Title</ListBoxItem>
              <ListBoxItem id="recent">Recent</ListBoxItem>
              <ListBoxItem id="playtime">Playtime</ListBoxItem>
            </ListBox>
          </Popover>
        </Select>
        <TextField aria-label="Search">
          <Label>Search</Label>
          <Input data-rect="300 380 300 40" />
        </TextField>
      </FocusSection>
    </>
  );
}

const focused = () => document.activeElement?.textContent ?? "";

describe.each(["pad", "keyboard"] as const)("with the %s only", (driver) => {
  function mount() {
    const pad = padSource();
    const back = vi.fn();
    render(
      <InputProvider console subscribe={pad.subscribe} onBack={back}>
        <Layout />
      </InputProvider>,
    );
    const press = (action: Action) => {
      if (driver === "pad") pad.press(action);
      else
        act(() => {
          fireEvent.keyDown(document.activeElement ?? document.body, { key: keys[action] });
        });
    };
    return { press, back };
  }

  it("moves between sidebar and content, through the shelf and the grid", () => {
    const { press } = mount();
    // Nothing focused: the content's first focusable.
    press("nav.down");
    expect(focused()).toBe("Shelf A");
    press("nav.right");
    expect(focused()).toBe("Shelf B");
    press("nav.down");
    expect(focused()).toBe("Card 1");
    press("nav.right");
    press("nav.down");
    expect(focused()).toBe("Card 5");
    // Along the row and into the sidebar; coming back, the content
    // returns to the card focus left it on, not the nearest one.
    press("nav.left");
    press("nav.left");
    expect(focused()).toBe("Card 3");
    press("nav.left");
    expect(focused()).toBe("Settings");
    press("nav.up");
    press("nav.up");
    expect(focused()).toBe("Home");
    press("nav.right");
    expect(focused()).toBe("Card 3");
  });

  it("goes across a list row to its button, not to a filter above or the next row's wider title", () => {
    const pad = padSource();
    render(
      <InputProvider console subscribe={pad.subscribe}>
        <Button data-rect="140 -60 120 40">Installed</Button>
        <Button data-rect="0 0 120 40">Game 1</Button>
        <Button data-rect="1500 0 40 40">Actions 1</Button>
        <Button data-rect="0 44 124 40">Game 2</Button>
        <Button data-rect="1500 44 40 40">Actions 2</Button>
      </InputProvider>,
    );
    act(() => screen.getByText("Game 1").focus());
    pad.press("nav.right");
    expect(focused()).toBe("Actions 1");
    pad.press("nav.down");
    expect(focused()).toBe("Actions 2");
    pad.press("nav.left");
    expect(focused()).toBe("Game 2");
  });

  it("opens and closes a dialog, staying inside it while open", async () => {
    const { press, back } = mount();
    act(() => screen.getByText("Open dialog").focus());
    press("confirm");
    await screen.findByRole("dialog");
    // Focus starts on the dialog's first control, not the dialog itself.
    await waitFor(() => expect(focused()).toBe("Yes"));
    press("nav.right");
    expect(focused()).toBe("No");
    // Nothing outside the dialog is reachable.
    press("nav.left");
    press("nav.left");
    expect(focused()).toBe("Yes");
    press("back");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(back).not.toHaveBeenCalled();
    await waitFor(() => expect(focused()).toBe("Open dialog"));
    // With no overlay, back goes back.
    press("back");
    expect(back).toHaveBeenCalledTimes(1);
  });

  it("opens a select with confirm, picks with the pad and closes with back", async () => {
    const { press } = mount();
    const trigger = screen
      .getAllByRole("button")
      .find((b) => b.closest("[data-rect='460 320 140 40']"));
    act(() => trigger?.focus());
    press("confirm");
    await screen.findByRole("listbox");
    await waitFor(() => expect(document.activeElement?.getAttribute("role")).toBe("option"));
    act(() => screen.getByRole("option", { name: "Title" }).focus());
    press("nav.down");
    expect(focused()).toBe("Recent");
    press("confirm");
    await waitFor(() => expect(screen.queryByRole("listbox")).toBeNull());
    expect(trigger?.textContent).toContain("Recent");
    // Open again and leave with back: nothing changes.
    act(() => trigger?.focus());
    press("confirm");
    await screen.findByRole("listbox");
    press("back");
    await waitFor(() => expect(screen.queryByRole("listbox")).toBeNull());
    expect(trigger?.textContent).toContain("Recent");
  });
});

describe("text fields", () => {
  it("keep their typing keys; up/down leave and Escape steps out", () => {
    const pad = padSource();
    render(
      <InputProvider console subscribe={pad.subscribe} onBack={() => {}}>
        <Layout />
      </InputProvider>,
    );
    const field = screen.getByRole("textbox");
    act(() => field.focus());
    for (const key of ["a", "ArrowLeft", "ArrowRight", "Enter", "[", "i", "Backspace"]) {
      const event = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true });
      expect(keyAction(Object.defineProperty(event, "target", { value: field }))).toBeNull();
      expect(fireEvent.keyDown(field, { key })).toBe(true); // not prevented
    }
    act(() => {
      fireEvent.keyDown(field, { key: "Escape" });
    });
    expect(document.activeElement).not.toBe(field);
    act(() => field.focus());
    act(() => {
      fireEvent.keyDown(field, { key: "ArrowUp" });
    });
    expect(focused()).toBe("Open dialog");
  });
});

describe("desktop mode", () => {
  it("leaves the keyboard alone until a pad is used", () => {
    const pad = padSource();
    render(
      <InputProvider subscribe={pad.subscribe}>
        <Layout />
      </InputProvider>,
    );
    act(() => screen.getByText("Card 0").focus());
    expect(fireEvent.keyDown(document.activeElement as Element, { key: "ArrowRight" })).toBe(true);
    expect(focused()).toBe("Card 0");
    pad.press("nav.right");
    expect(focused()).toBe("Card 1");
    // After a pad action the keyboard map is on too.
    act(() => {
      fireEvent.keyDown(document.activeElement as Element, { key: "ArrowRight" });
    });
    expect(focused()).toBe("Card 2");
  });
});

function Bound({ onShelf, onView }: { onShelf: () => void; onView: () => void }) {
  const shelf = useRef<HTMLDivElement>(null);
  useInputAction("tab.next", onView, { label: "Next tab" });
  useInputAction("tab.next", onShelf, { label: "Next page", owner: shelf });
  useInputAction("details", () => {}, { label: "Details" });
  const hints = useActiveBindings();
  return (
    <>
      <FocusSection id="shelf" ref={shelf}>
        <Button data-rect="0 0 100 40">In shelf</Button>
      </FocusSection>
      <Button data-rect="0 100 100 40">Outside</Button>
      <output aria-label="hints">{hints.map((h) => `${h.action}=${h.label}`).join(",")}</output>
    </>
  );
}

function WithDialog({ onBack }: { onBack: () => void }) {
  useInputAction("back", onBack);
  return <Layout />;
}

function Views() {
  const [view, setView] = useState("grid");
  return (
    <InputProvider console locationKey={view} subscribe={async () => () => {}}>
      <FocusSection id="main">
        {view === "grid" ? (
          [0, 1, 2].map((i) => (
            <Button key={i} data-rect={`${i * 150} 0 140 80`} onPress={() => setView("details")}>
              Card {i}
            </Button>
          ))
        ) : (
          <Button data-rect="0 0 140 40" onPress={() => setView("grid")}>
            Back to grid
          </Button>
        )}
      </FocusSection>
    </InputProvider>
  );
}

function Custom() {
  const { ref, focused: isFocused } = useFocusable<HTMLDivElement>({ focusKey: "tile" });
  return (
    <div ref={ref} data-rect="0 0 100 100">
      {isFocused ? "focused tile" : "tile"}
    </div>
  );
}

describe("command bindings", () => {
  it("lets the innermost binding win and lists the active ones", () => {
    const pad = padSource();
    const onShelf = vi.fn();
    const onView = vi.fn();
    render(
      <InputProvider console subscribe={pad.subscribe}>
        <Bound onShelf={onShelf} onView={onView} />
      </InputProvider>,
    );
    act(() => screen.getByText("In shelf").focus());
    pad.press("tab.next");
    expect(onShelf).toHaveBeenCalledTimes(1);
    expect(onView).not.toHaveBeenCalled();
    expect(screen.getByLabelText("hints").textContent).toBe("tab.next=Next page,details=Details");
    act(() => screen.getByText("Outside").focus());
    pad.press("tab.next");
    expect(onView).toHaveBeenCalledTimes(1);
    expect(screen.getByLabelText("hints").textContent).toBe("details=Details,tab.next=Next tab");
  });

  it("ignores the view's bindings while an overlay is open", async () => {
    const pad = padSource();
    const onBack = vi.fn();
    render(
      <InputProvider console subscribe={pad.subscribe}>
        <WithDialog onBack={onBack} />
      </InputProvider>,
    );
    act(() => screen.getByText("Open dialog").focus());
    pad.press("confirm");
    await screen.findByRole("dialog");
    pad.press("back");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(onBack).not.toHaveBeenCalled();
    pad.press("back");
    expect(onBack).toHaveBeenCalledTimes(1);
  });
});

describe("focus restore", () => {
  it("puts focus back on the card that opened a view", async () => {
    render(<Views />);
    await waitFor(() => expect(focused()).toBe("Card 0"));
    act(() => screen.getByText("Card 2").focus());
    act(() => screen.getByText("Card 2").click());
    await waitFor(() => expect(focused()).toBe("Back to grid"));
    act(() => screen.getByText("Back to grid").click());
    await waitFor(() => expect(focused()).toBe("Card 2"));
  });

  it("makes custom elements focus stops", () => {
    const pad = padSource();
    render(
      <InputProvider console subscribe={pad.subscribe}>
        <FocusSection id="main">
          <Custom />
        </FocusSection>
      </InputProvider>,
    );
    pad.press("nav.down");
    expect(focused()).toBe("focused tile");
    expect(document.activeElement?.getAttribute("data-focus-key")).toBe("tile");
  });
});

function Controls() {
  return (
    <FocusSection id="main">
      <Switch data-rect="0 0 200 40">Effects</Switch>
      <Checkbox data-rect="0 50 200 40">Titles</Checkbox>
      <Slider aria-label="Scale" defaultValue={50}>
        <SliderTrack data-rect="0 100 200 40">
          <SliderThumb data-rect="90 100 20 40" />
        </SliderTrack>
      </Slider>
      <RadioGroup aria-label="Density" defaultValue="compact">
        <Radio value="compact" data-rect="0 150 100 40">
          Compact
        </Radio>
        <Radio value="comfy" data-rect="110 150 100 40">
          Comfy
        </Radio>
      </RadioGroup>
    </FocusSection>
  );
}

describe("form controls", () => {
  it("toggle, slide and choose with actions only", () => {
    const pad = padSource();
    render(
      <InputProvider console subscribe={pad.subscribe}>
        <Controls />
      </InputProvider>,
    );
    pad.press("nav.down");
    const effects = screen.getByRole("switch");
    expect(document.activeElement).toBe(effects);
    pad.press("confirm");
    expect((effects as HTMLInputElement).checked).toBe(true);
    pad.press("nav.down");
    const titles = screen.getByRole("checkbox");
    expect(document.activeElement).toBe(titles);
    pad.press("confirm");
    expect((titles as HTMLInputElement).checked).toBe(true);
    pad.press("nav.down");
    const scale = screen.getByRole("slider");
    expect(document.activeElement).toBe(scale);
    pad.press("nav.right");
    expect((scale as HTMLInputElement).value).toBe("51");
    pad.press("nav.left");
    pad.press("nav.left");
    expect((scale as HTMLInputElement).value).toBe("49");
    pad.press("nav.down");
    expect(document.activeElement).toBe(screen.getByRole("radio", { name: "Compact" }));
    pad.press("nav.right");
    pad.press("confirm");
    expect((screen.getByRole("radio", { name: "Comfy" }) as HTMLInputElement).checked).toBe(true);
  });
});

it("up and down stay in the area focus is in, and left and right cross over", async () => {
  const pad = padSource();
  render(
    <InputProvider console subscribe={pad.subscribe}>
      <FocusSection id="sidebar">
        <Button data-rect="20 100 260 50">Home</Button>
        <Button data-rect="20 750 260 60">Desktop mode</Button>
      </FocusSection>
      <FocusSection id="main">
        {/* At the content's left edge, the sidebar's last item is nearer
            below than the switch at the end of the next row. */}
        <Button data-rect="340 334 150 40">Reset</Button>
        <Button data-rect="780 534 40 20">Switch</Button>
      </FocusSection>
    </InputProvider>,
  );
  const button = (name: string) => screen.getByRole("button", { name });
  act(() => button("Reset").focus());
  pad.press("nav.down");
  expect(document.activeElement).toBe(button("Switch"));
  // With nothing further down in the content, down crosses over.
  pad.press("nav.down");
  expect(document.activeElement).toBe(button("Desktop mode"));
  pad.press("nav.right");
  expect(document.activeElement).toBe(button("Switch"));
});

it("down from the hero goes to the shelf below it, not a card in its column further down", () => {
  const pad = padSource();
  render(
    <InputProvider console subscribe={pad.subscribe}>
      <Button data-rect="560 380 260 56">Resume · View details</Button>
      {/* Recently Played: its View all at the right, one card at the left. */}
      <Button data-rect="1400 480 120 30">View all →</Button>
      <Button data-rect="340 530 200 280">Pocket Quest</Button>
      {/* Recently Added: its second card is under the hero's button. */}
      <Button data-rect="1400 860 120 30">View all → added</Button>
      <Button data-rect="340 910 200 280">Pocket Quest added</Button>
      <Button data-rect="560 910 200 280">Couch Runner</Button>
    </InputProvider>,
  );
  act(() => screen.getByText("Resume · View details").focus());
  pad.press("nav.down");
  expect(focused()).toBe("View all →");
  pad.press("nav.down");
  expect(focused()).toBe("Pocket Quest");
  // Up is the same: the line just above, its shelf's View all.
  act(() => screen.getByText("Couch Runner").focus());
  pad.press("nav.up");
  expect(focused()).toBe("View all → added");
});

it("a settings row is one stop going up and down, wherever its control sits", async () => {
  const pad = padSource();
  render(
    <InputProvider console subscribe={pad.subscribe}>
      <FocusSection id="main">
        <Button data-rect="340 334 150 40">Reset</Button>
        <div data-cs="setting-row" data-rect="340 500 520 60">
          <Button data-rect="800 515 40 20">Launch on startup</Button>
        </div>
        <div data-cs="setting-row" data-rect="340 580 520 60">
          <Button data-rect="600 595 120 30">Open to</Button>
          <Button data-rect="740 595 100 30">Second</Button>
        </div>
      </FocusSection>
    </InputProvider>,
  );
  const button = (name: string) => screen.getByRole("button", { name });
  act(() => button("Reset").focus());
  pad.press("nav.down");
  expect(document.activeElement).toBe(button("Launch on startup"));
  pad.press("nav.down");
  expect(document.activeElement).toBe(button("Open to"));
  // Within a row, the controls' own places.
  pad.press("nav.right");
  expect(document.activeElement).toBe(button("Second"));
  pad.press("nav.up");
  expect(document.activeElement).toBe(button("Launch on startup"));
});
