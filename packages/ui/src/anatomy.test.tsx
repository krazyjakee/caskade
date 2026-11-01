import { cleanup, render } from "@testing-library/react";
import { Gauge, Play, Sparkles } from "lucide-react";
import type { ReactElement } from "react";
import { MenuTrigger } from "react-aria-components";
import { afterEach, describe, expect, it } from "vitest";
import { Avatar, AvatarPicker } from "./Avatar";
import { type AnatomyEntry, type AnatomyName, anatomy, checkAnatomy, ownedParts } from "./anatomy";
import { ErrorNotice } from "./ErrorNotice";
import { Button } from "./form/Button";
import { Checkbox } from "./form/Checkbox";
import { SegmentedControl } from "./form/SegmentedControl";
import { Select } from "./form/Select";
import { Pill, SettingRow, SettingsSection, StatusDot } from "./form/SettingsSection";
import { Slider } from "./form/Slider";
import { Switch } from "./form/Switch";
import { TextField } from "./form/TextField";
import { GameArt } from "./GameArt";
import { type CardGame, GameCard } from "./GameCard";
import { Hero } from "./Hero";
import { ContextMenu, Menu, MenuItem, MenuPopover, MenuSection, MenuSeparator } from "./Menu";
import { Shelf } from "./Shelf";
import { type GameStatus, StatusBadge } from "./StatusBadge";

afterEach(cleanup);

type UiName = {
  [K in AnatomyName]: (typeof anatomy)[K]["source"] extends "@caskade/ui" ? K : never;
}[AnatomyName];

/** Interaction attributes a test can set through props. Hover, press, focus
 * and transitions need real pointer and keyboard input. */
const settable = [
  "data-disabled",
  "data-selected",
  "data-indeterminate",
  "data-invalid",
  "data-readonly",
  "data-required",
  "data-open",
  "data-orientation",
  "data-pending",
];

interface Fixture {
  /** A render that shows every part the package renders, between its roots. */
  variants: ReactElement;
  /** A render with the root's state attribute set to `value`. */
  state?: (attribute: string, value: string) => ReactElement;
  /** A render with each settable interaction attribute on the root. */
  interaction?: Record<string, ReactElement>;
}

const noop = () => {};
const options = [
  { id: "grid", label: "Grid", icon: Gauge },
  { id: "list", label: "List" },
] as const;
const game: CardGame = {
  title: "Nebula Drift",
  genre: "Racing",
  status: "installed",
  art: [220, 280],
  playtimeHours: 12,
  lastPlayed: "Yesterday",
  sizeGb: 18,
};
const hero = { ...game, art: [220, 280] as [number, number], description: "Race the void." };

const fixtures: Record<UiName, Fixture> = {
  button: {
    variants: <Button icon={Play}>Play</Button>,
    state: (attribute, value) =>
      attribute === "data-variant" ? (
        <Button variant={value as "primary"}>Play</Button>
      ) : (
        <Button size={value as "sm"}>Play</Button>
      ),
    interaction: {
      "data-disabled": <Button isDisabled>Play</Button>,
      "data-pending": <Button isPending>Play</Button>,
    },
  },
  checkbox: {
    variants: <Checkbox>Hide installed</Checkbox>,
    interaction: {
      "data-selected": <Checkbox defaultSelected>Hide installed</Checkbox>,
      "data-indeterminate": <Checkbox isIndeterminate>Hide installed</Checkbox>,
      "data-disabled": <Checkbox isDisabled>Hide installed</Checkbox>,
      "data-invalid": <Checkbox isInvalid>Hide installed</Checkbox>,
      "data-readonly": <Checkbox isReadOnly>Hide installed</Checkbox>,
    },
  },
  segmented: {
    variants: (
      <SegmentedControl aria-label="Layout" options={options} value="grid" onChange={noop} />
    ),
    interaction: {
      "data-disabled": (
        <SegmentedControl
          aria-label="Layout"
          isDisabled
          options={options}
          value="grid"
          onChange={noop}
        />
      ),
      "data-orientation": (
        <SegmentedControl aria-label="Layout" options={options} value="grid" onChange={noop} />
      ),
    },
  },
  select: {
    variants: <Select label="Sort" options={options} value="grid" onChange={noop} />,
    interaction: {
      "data-open": (
        <Select label="Sort" defaultOpen options={options} value="grid" onChange={noop} />
      ),
      "data-disabled": (
        <Select label="Sort" isDisabled options={options} value="grid" onChange={noop} />
      ),
      "data-invalid": (
        <Select label="Sort" isInvalid options={options} value="grid" onChange={noop} />
      ),
    },
  },
  "select-popover": {
    variants: <Select label="Sort" defaultOpen options={options} value="grid" onChange={noop} />,
  },
  menu: {
    variants: (
      <>
        <MenuTrigger defaultOpen>
          <Button>Add game</Button>
          <MenuPopover>
            <Menu aria-label="Add game">
              <MenuItem>From a file…</MenuItem>
            </Menu>
          </MenuPopover>
        </MenuTrigger>
        <ContextMenu aria-label="Nebula Drift" anchor={{ x: 10, y: 10 }} onClose={noop}>
          <MenuSection title="Game">
            <MenuItem icon={Play}>Play</MenuItem>
          </MenuSection>
          <MenuSeparator />
          <MenuSection>
            <MenuItem tone="danger" description="Scans won't bring it back.">
              Hide from library
            </MenuItem>
          </MenuSection>
        </ContextMenu>
      </>
    ),
  },
  slider: {
    variants: <Slider label="Volume" defaultValue={40} />,
    interaction: {
      "data-disabled": <Slider label="Volume" isDisabled defaultValue={40} />,
      "data-orientation": <Slider label="Volume" defaultValue={40} />,
    },
  },
  switch: {
    variants: <Switch>Launch on login</Switch>,
    state: (_, value) => <Switch size={value as "sm"}>Launch on login</Switch>,
    interaction: {
      "data-selected": <Switch defaultSelected>Launch on login</Switch>,
      "data-disabled": <Switch isDisabled>Launch on login</Switch>,
      "data-readonly": <Switch isReadOnly>Launch on login</Switch>,
    },
  },
  "text-field": {
    variants: (
      <TextField
        label="Path"
        description="Where games go."
        icon={Sparkles}
        action={<Button>Browse…</Button>}
        validationBehavior="aria"
        validate={() => "Not a folder"}
      />
    ),
    interaction: {
      "data-disabled": <TextField label="Path" isDisabled />,
      "data-invalid": <TextField label="Path" isInvalid />,
      "data-readonly": <TextField label="Path" isReadOnly />,
      "data-required": <TextField label="Path" isRequired />,
    },
  },
  "settings-section": {
    variants: (
      <SettingsSection
        icon={Gauge}
        title="Performance"
        description="How hard the app works."
        aside={
          <Pill tone="accent" icon={Sparkles}>
            New
          </Pill>
        }
      >
        <SettingRow label="Limit frame rate">
          <Switch />
        </SettingRow>
      </SettingsSection>
    ),
  },
  "setting-row": {
    variants: (
      <SettingRow icon={Gauge} label="Limit frame rate" description="Saves power.">
        <Switch />
      </SettingRow>
    ),
  },
  "status-dot": {
    variants: <StatusDot tone="ok">Detected</StatusDot>,
    state: (_, value) => <StatusDot tone={value as "ok"}>Detected</StatusDot>,
  },
  pill: {
    variants: (
      <Pill tone="warning" icon={Sparkles}>
        Caution
      </Pill>
    ),
    state: (_, value) => (
      <Pill tone={value as "warning"} icon={Sparkles}>
        Caution
      </Pill>
    ),
  },
  "game-art": {
    variants: (
      <>
        <GameArt game={game} />
        <GameArt game={game} image="cover.webp" />
      </>
    ),
    state: (_, value) => <GameArt game={game} image={value === "image" ? "cover.webp" : null} />,
  },
  "status-badge": {
    variants: <StatusBadge status="installed" />,
    state: (_, value) => <StatusBadge status={value as GameStatus} />,
  },
  "game-card": {
    variants: (
      <>
        <GameCard game={game} />
        <GameCard game={game} variant="recent" />
        <GameCard game={{ ...game, stores: ["steam"] }} variant="store" />
        <GameCard game={game} variant="store" />
      </>
    ),
    state: (attribute, value) =>
      attribute === "data-status" ? (
        <GameCard game={{ ...game, status: value as GameStatus }} />
      ) : (
        <GameCard game={game} variant={value as "store"} />
      ),
  },
  hero: {
    variants: <Hero game={hero} eyebrow="Continue playing" action="Play" />,
  },
  shelf: {
    variants: (
      <Shelf title="Recently played" icon={Gauge} subtitle="This week">
        <GameCard game={game} />
      </Shelf>
    ),
  },
  avatar: {
    variants: (
      <>
        <Avatar avatar="avatar:rocket" name="Ada" />
        <Avatar avatar="avatar:removed" name="Ada Lovelace" />
      </>
    ),
    state: (attribute, value) =>
      attribute === "data-size" ? (
        <Avatar avatar="avatar:rocket" name="Ada" size={value as "sm"} />
      ) : (
        <Avatar avatar={value === "image" ? "avatar:rocket" : "avatar:removed"} name="Ada" />
      ),
  },
  "error-notice": {
    variants: (
      <ErrorNotice
        title="Couldn't load your library"
        summary="The library database couldn't be read."
        hints={["Restart Caskade."]}
        detail="database is locked"
        reference="1a2b3c4d"
        actions={<Button>Try again</Button>}
        defaultExpanded
      />
    ),
    state: (attribute, value) =>
      attribute === "data-variant" ? (
        <ErrorNotice summary="Offline" variant={value as "block"} detail="timed out" />
      ) : attribute === "data-tone" ? (
        <ErrorNotice summary="Offline" tone={value as "danger"} detail="timed out" />
      ) : (
        <ErrorNotice summary="Offline" detail="timed out" defaultExpanded={value === "true"} />
      ),
  },
  "avatar-picker": {
    variants: <AvatarPicker value="avatar:rocket" onChange={noop} />,
    state: (_, value) => <AvatarPicker value={null} onChange={noop} size={value as "md"} />,
    interaction: {
      "data-disabled": <AvatarPicker isDisabled value={null} onChange={noop} />,
      "data-orientation": <AvatarPicker value={null} onChange={noop} />,
    },
  },
};

/** The catalogued roots called `name` in the document (popovers render in a portal). */
const roots = (name: string) => [...document.body.querySelectorAll(`[data-cs="${name}"]`)];

const entries = Object.entries(anatomy).filter(
  (entry): entry is [UiName, (typeof anatomy)[UiName]] => entry[1].source === "@caskade/ui",
);

it("has a fixture for every @caskade/ui entry in the catalogue", () => {
  expect(Object.keys(fixtures).sort()).toEqual(entries.map(([name]) => name).sort());
});

describe.each(entries)("%s", (name, entry: AnatomyEntry) => {
  const fixture = fixtures[name];
  const appOnly = new Set(entry.appOnly ?? []);

  it("renders its root, only catalogued parts, and every part between its variants", () => {
    const seen = new Set<string>();
    render(fixture.variants);
    const found = roots(name);
    expect(found.length, "root").toBeGreaterThan(0);
    for (const root of found) {
      expect(checkAnatomy(root)).toEqual([]);
      for (const part of ownedParts(root)) {
        const value = part.getAttribute("data-part") ?? "";
        if (value in entry.parts) seen.add(value);
      }
    }
    const expected = Object.keys(entry.parts).filter((part) => !appOnly.has(part));
    expect([...seen].sort()).toEqual(expected.sort());
  });

  const states = Object.entries(entry.states)
    .filter(([attribute]) => !appOnly.has(attribute))
    .flatMap(([attribute, values]) => values.map((value) => [attribute, value] as const));
  if (states.length)
    it.each(states)("sets %s=%s", (attribute, value) => {
      if (!fixture.state) throw new Error(`${name} has states but no state fixture`);
      render(fixture.state(attribute, value));
      const [root] = roots(name);
      expect(root?.getAttribute(attribute)).toBe(value);
      expect(checkAnatomy(root as Element)).toEqual([]);
    });

  const interactions = entry.interaction.filter((attribute) => settable.includes(attribute));
  if (interactions.length)
    it.each(interactions)("carries React Aria's %s", (attribute) => {
      const element = fixture.interaction?.[attribute];
      if (!element) throw new Error(`${name} lists ${attribute} but has no fixture for it`);
      render(element);
      expect(roots(name)[0]?.hasAttribute(attribute)).toBe(true);
    });
});

it("reports undocumented parts and state values", () => {
  const { container } = render(
    <div data-cs="status-badge" data-status="lost">
      <span data-part="sticker" />
      <div data-cs="game-art" data-part="icon">
        <span data-part="label" />
      </div>
    </div>,
  );
  expect(checkAnatomy(container.firstElementChild as Element)).toEqual([
    'status-badge: undocumented part "sticker"',
    'status-badge: data-status="lost" is not one of installed, not-installed, available, emulated',
  ]);
  // Content in a slot is the caller's, not the row's.
  const row = render(
    <div data-cs="setting-row">
      <div data-part="controls">
        <span data-part="caller-thing" />
      </div>
    </div>,
  );
  expect(checkAnatomy(row.container.firstElementChild as Element)).toEqual([]);
  expect(checkAnatomy(document.createElement("div"))).toEqual([
    '"" is not in the anatomy catalogue',
  ]);
});
