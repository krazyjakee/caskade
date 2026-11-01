import { i18n } from "@caskade/i18n";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { Input, Label, TextField } from "react-aria-components";
import { afterEach, expect, it, vi } from "vitest";
import type { Action } from "./actions";
import { InputProvider, type PadAction } from "./index";

afterEach(cleanup);

function padSource() {
  let send: (action: PadAction) => void = () => {};
  return {
    subscribe: async (listener: (action: PadAction) => void) => {
      send = listener;
      return () => {};
    },
    press: (action: Action) => act(() => send({ action, glyphs: "xbox" })),
  };
}

/** A controlled field in a form, as the app's fields are. */
function Form({ onSubmit }: { onSubmit: (value: string) => void }) {
  const [value, setValue] = useState("");
  return (
    <form
      onSubmit={(event) => {
        event.preventDefault();
        onSubmit(value);
      }}
    >
      <TextField value={value} onChange={setValue}>
        <Label>Name</Label>
        <Input />
      </TextField>
      <output aria-label="value">{value}</output>
    </form>
  );
}

function mount() {
  const pad = padSource();
  const onSubmit = vi.fn();
  render(
    <InputProvider console subscribe={pad.subscribe}>
      <Form onSubmit={onSubmit} />
    </InputProvider>,
  );
  const field = screen.getByRole("textbox", { name: "Name" }) as HTMLInputElement;
  const value = () => screen.getByRole("status", { name: "value" }).textContent;
  const keyboard = () => document.querySelector('[data-cs="osk"]');
  return { pad, onSubmit, field, value, keyboard };
}

it("A on a field opens the keyboard; the pad types, deletes, spaces and moves the cursor", async () => {
  const { pad, field, value, keyboard } = mount();
  act(() => field.focus());
  pad.press("confirm");
  expect(keyboard()).not.toBeNull();
  // It starts on q; A types it into the field, which keeps focus.
  pad.press("confirm");
  pad.press("nav.right");
  pad.press("confirm");
  expect(value()).toBe("qw");
  // Up from w is 2: the columns line up.
  pad.press("nav.up");
  pad.press("confirm");
  expect(value()).toBe("qw2");
  pad.press("details");
  expect(value()).toBe("qw");
  pad.press("search");
  expect(value()).toBe("qw ");
  // RB: symbols, on the same key position (@ above w's column).
  pad.press("tab.next");
  expect(keyboard()?.getAttribute("data-layer")).toBe("symbols");
  pad.press("confirm");
  expect(value()).toBe("qw @");
  // LT moves the cursor back; typing goes in there.
  pad.press("page.prev");
  pad.press("page.prev");
  pad.press("confirm");
  expect(value()).toBe("qw@ @");
  expect(document.activeElement).toBe(field);
});

it("shift types one capital, twice locks; Start submits and closes", async () => {
  const { pad, field, value, keyboard, onSubmit } = mount();
  act(() => field.focus());
  pad.press("confirm");
  act(() => (screen.getByText("⇧") as HTMLElement).click());
  expect(keyboard()?.getAttribute("data-shift")).toBe("once");
  // Up from shift to s, then up again to w.
  pad.press("nav.up");
  pad.press("nav.up");
  // Once in capitals, then lower case again.
  pad.press("confirm");
  pad.press("confirm");
  expect(value()).toBe("Ww");
  expect(keyboard()?.getAttribute("data-shift")).toBe("off");
  const shift = screen.getByText("⇧") as HTMLElement;
  act(() => shift.click());
  act(() => shift.click());
  expect(keyboard()?.getAttribute("data-shift")).toBe("lock");
  // The click moved the pad's place to shift; back up to w.
  pad.press("nav.up");
  pad.press("nav.up");
  pad.press("confirm");
  pad.press("confirm");
  expect(value()).toBe("WwWW");
  pad.press("menu");
  expect(onSubmit).toHaveBeenCalledWith("WwWW");
  await waitFor(() => expect(keyboard()).toBeNull());
});

it("B closes the keyboard and keeps the text; leaving the field closes it", () => {
  const { pad, field, value, keyboard } = mount();
  act(() => field.focus());
  pad.press("confirm");
  pad.press("confirm");
  pad.press("back");
  expect(keyboard()).toBeNull();
  expect(value()).toBe("q");
  expect(document.activeElement).toBe(field);
  pad.press("confirm");
  expect(keyboard()).not.toBeNull();
  act(() => field.blur());
  expect(keyboard()).toBeNull();
});

it("keys press Enter on a field, without the on-screen keyboard", () => {
  const { field, keyboard, onSubmit } = mount();
  act(() => field.focus());
  fireEvent.keyDown(field, { key: "Enter" });
  expect(keyboard()).toBeNull();
  // The field's own Enter (a native submit) is the browser's, not ours.
  expect(onSubmit).not.toHaveBeenCalled();
});

it("a tap in console mode opens it", () => {
  const { field, keyboard } = mount();
  act(() => field.focus());
  act(() => {
    const tap = new Event("pointerup", { bubbles: true }) as PointerEvent;
    Object.defineProperty(tap, "pointerType", { value: "touch" });
    field.dispatchEvent(tap);
  });
  expect(keyboard()).not.toBeNull();
});

/** Switches the language as the app does; the keyboard follows it. */
function inLanguage(locale: string) {
  i18n.loadAndActivate({ locale, messages: {} });
}
const keys = (row: number) =>
  Array.from(
    document
      .querySelectorAll('[data-cs="osk"] [data-part="row"]')
      [row]?.querySelectorAll('[data-part="key"]') ?? [],
    (key) => key.getAttribute("data-key"),
  ).join("");

it("is AZERTY in French, and a pad types é, è, à and ç", () => {
  inLanguage("fr");
  const { pad, field, value } = mount();
  act(() => field.focus());
  pad.press("confirm");
  expect(keys(1)).toBe("azertyuiopéè");
  expect(keys(2)).toBe("qsdfghjklmàç");
  // It starts on a; left wraps round to è, then é.
  pad.press("confirm");
  pad.press("nav.left");
  pad.press("confirm");
  pad.press("nav.left");
  pad.press("confirm");
  // Down from é is à, and ç is beside it.
  pad.press("nav.down");
  pad.press("confirm");
  pad.press("nav.right");
  pad.press("confirm");
  expect(value()).toBe("aèéàç");
  // Shift gives the capital: from shift up to s, z, then left to é.
  act(() => (screen.getByText("⇧") as HTMLElement).click());
  pad.press("nav.up");
  pad.press("nav.up");
  for (let i = 0; i < 3; i++) pad.press("nav.left");
  pad.press("confirm");
  expect(value()).toBe("aèéàçÉ");
  // The other accents are a row of the symbols.
  pad.press("tab.next");
  expect(keys(3)).toBe("âêîôûëïüùœ:_");
});

it("is QWERTZ in German and has å, ä and ö in Swedish", () => {
  inLanguage("de-AT");
  const german = mount();
  act(() => german.field.focus());
  german.pad.press("confirm");
  expect(keys(1)).toBe("qwertzuiopü-");
  expect(keys(2)).toBe("asdfghjklöä.");
  expect(keys(3)).toBe("shiftyxcvbnm,ß'");
  cleanup();

  inLanguage("sv");
  const { pad, field, value } = mount();
  act(() => field.focus());
  pad.press("confirm");
  expect(keys(1)).toBe("qwertyuiopå-");
  // From q, left twice wraps round to å; down to ä, left to ö.
  pad.press("nav.left");
  pad.press("nav.left");
  pad.press("confirm");
  pad.press("nav.down");
  pad.press("confirm");
  pad.press("nav.left");
  pad.press("confirm");
  expect(value()).toBe("åäö");
});

it("is English's QWERTY in a language without a layout of its own", () => {
  inLanguage("ja");
  const { pad, field } = mount();
  act(() => field.focus());
  pad.press("confirm");
  expect(keys(1)).toBe("qwertyuiop-_");
});
