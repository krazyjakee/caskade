/** How each @caskade/ui entry of the anatomy catalogue renders: every part,
 * each domain state value and each interaction state a prop can set. The
 * component stories are generated from these and the catalogue; the same
 * renders back `packages/ui/src/anatomy.test.tsx`. */
import {
  type AnatomyName,
  Avatar,
  AvatarPicker,
  anatomy,
  Button,
  type CardGame,
  Checkbox,
  ErrorNotice,
  GameArt,
  GameCard,
  type GameStatus,
  Hero,
  type HeroGame,
  Menu,
  MenuItem,
  MenuPopover,
  MenuSection,
  MenuSeparator,
  Pill,
  SegmentedControl,
  Select,
  SettingRow,
  SettingsSection,
  Shelf,
  Slider,
  StatusBadge,
  StatusDot,
  Switch,
  TextField,
} from "@caskade/ui";
import { EyeOff, Gauge, LayoutGrid, Play, Sparkles } from "lucide-react";
import type { ReactElement } from "react";
import { MenuTrigger } from "react-aria-components";

/** The catalogue entries @caskade/ui renders. App entries (the shell, the
 * library's cards) are rendered and checked by `apps/desktop`. */
export type UiName = {
  [K in AnatomyName]: (typeof anatomy)[K]["source"] extends "@caskade/ui" ? K : never;
}[AnatomyName];

export const uiNames = (Object.keys(anatomy) as AnatomyName[]).filter(
  (name): name is UiName => anatomy[name].source === "@caskade/ui",
);

export interface Fixture {
  /** A render that shows every part the package renders, between its roots. */
  variants: ReactElement;
  /** A render with the root's state attribute set to `value`. */
  state?: (attribute: string, value: string) => ReactElement;
  /** Interaction attributes props can set (hover, press and focus need a
   * pointer or keyboard: try them in the canvas). */
  interaction?: Record<string, ReactElement>;
}

const noop = () => {};
const options = [
  { id: "grid", label: "Grid", icon: LayoutGrid },
  { id: "list", label: "List" },
] as const;

export const games: readonly CardGame[] = [
  {
    title: "Nebula Drift",
    genre: "Racing",
    status: "installed",
    art: [220, 280],
    playtimeHours: 12,
    lastPlayed: "Yesterday",
    sizeGb: 18,
  },
  {
    title: "Hollow Crown",
    genre: "Action RPG",
    status: "not-installed",
    art: [12, 340],
    playtimeHours: 64,
    lastPlayed: "Last week",
    sizeGb: 42,
  },
  {
    title: "Tidebreaker",
    genre: "Strategy",
    status: "available",
    art: [180, 140],
    sizeGb: 9,
    stores: ["steam", "gog"],
  },
  {
    title: "Pixel Paladin",
    genre: "SNES · Platformer",
    status: "emulated",
    art: [40, 300],
    playtimeHours: 3,
    lastPlayed: "Today",
    sizeGb: 1,
  },
];

const game = games[0] as CardGame;

/** Stands in for downloaded artwork (`data-source="image"`). */
const cover = `data:image/svg+xml,${encodeURIComponent(
  '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 160 100"><rect width="160" height="100" fill="#1d2b5a"/><circle cx="110" cy="35" r="22" fill="#38d9f5"/><path d="M0 100 60 40l40 40 20-20 40 40z" fill="#5b5ff7"/></svg>',
)}`;

export const hero: HeroGame = {
  ...game,
  art: [220, 280],
  description: "Race the void between collapsing stars.",
};

export const fixtures: Record<UiName, Fixture> = {
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
    },
  },
  select: {
    variants: <Select label="Sort" options={options} value="grid" onChange={noop} />,
    interaction: {
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
      <MenuTrigger defaultOpen>
        <Button>Nebula Drift</Button>
        <MenuPopover>
          <Menu aria-label="Nebula Drift">
            <MenuSection title="Game">
              <MenuItem icon={Play}>Play</MenuItem>
            </MenuSection>
            <MenuSeparator />
            <MenuSection>
              <MenuItem icon={EyeOff} tone="danger" description="Scans won't bring it back.">
                Hide from library
              </MenuItem>
            </MenuSection>
          </Menu>
        </MenuPopover>
      </MenuTrigger>
    ),
  },
  slider: {
    variants: <Slider label="Volume" defaultValue={40} />,
    interaction: {
      "data-disabled": <Slider label="Volume" isDisabled defaultValue={40} />,
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
      <div className="grid w-80 grid-cols-2 gap-4">
        <GameArt game={game} className="aspect-(--cs-card-aspect)" />
        <GameArt game={game} image={cover} className="aspect-(--cs-card-aspect)" />
      </div>
    ),
    state: (_, value) => (
      <GameArt
        game={game}
        image={value === "image" ? cover : null}
        className="aspect-(--cs-card-aspect) w-36"
      />
    ),
  },
  "status-badge": {
    variants: <StatusBadge status="installed" />,
    state: (_, value) => <StatusBadge status={value as GameStatus} />,
  },
  "game-card": {
    variants: (
      <div className="grid w-[48rem] grid-cols-4 gap-4">
        <GameCard game={game} />
        <GameCard game={game} variant="recent" />
        <GameCard game={{ ...game, stores: ["steam"] }} variant="store" />
        <GameCard game={game} variant="store" />
      </div>
    ),
    state: (attribute, value) => (
      <div className="w-48">
        {attribute === "data-status" ? (
          <GameCard game={{ ...game, status: value as GameStatus }} />
        ) : (
          <GameCard game={game} variant={value as "store"} />
        )}
      </div>
    ),
  },
  hero: {
    variants: <Hero game={hero} eyebrow="Continue playing" action="Play" />,
  },
  shelf: {
    variants: (
      <Shelf title="Recently played" icon={Gauge} subtitle="This week">
        {games.map((entry, index) => (
          <GameCard key={entry.title} game={entry} index={index} />
        ))}
      </Shelf>
    ),
  },
  avatar: {
    variants: (
      <div className="flex items-center gap-4">
        <Avatar avatar="avatar:astronaut" name="Ada" size="lg" />
        {/* An id a later release removed: initials instead. */}
        <Avatar avatar="avatar:retired" name="Ada Lovelace" size="lg" />
      </div>
    ),
    state: (attribute, value) =>
      attribute === "data-size" ? (
        <Avatar avatar="avatar:astronaut" name="Ada" size={value as "sm"} />
      ) : (
        <Avatar
          avatar={value === "image" ? "avatar:astronaut" : "avatar:retired"}
          name="Ada Lovelace"
          size="lg"
        />
      ),
  },
  "error-notice": {
    variants: (
      <div className="w-[32rem]">
        <ErrorNotice
          title="Couldn't load your library"
          summary="The library database couldn't be read."
          hints={["Restart Caskade."]}
          detail="database is locked"
          reference="1a2b3c4d"
          actions={<Button>Try again</Button>}
          defaultExpanded
        />
      </div>
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
    variants: (
      <div className="w-[40rem]">
        <AvatarPicker value="avatar:rocket" onChange={noop} />
      </div>
    ),
    state: (_, value) => (
      <div className="w-[40rem]">
        <AvatarPicker value="avatar:moon" onChange={noop} size={value as "md"} />
      </div>
    ),
    interaction: {
      "data-disabled": (
        <div className="w-[40rem]">
          <AvatarPicker isDisabled value="avatar:moon" onChange={noop} size="md" />
        </div>
      ),
      "data-orientation": (
        <div className="w-[40rem]">
          <AvatarPicker value="avatar:moon" onChange={noop} size="md" />
        </div>
      ),
    },
  },
};
