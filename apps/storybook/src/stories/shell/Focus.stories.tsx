import {
  Button,
  type CardGame,
  Checkbox,
  GameArt,
  GameCard,
  SegmentedControl,
  Select,
  SettingRow,
  SettingsSection,
  Slider,
  Switch,
  TextField,
} from "@caskade/ui";
import type { Meta } from "@storybook/react-vite";
import { FolderOpen, Gamepad2, LayoutGrid, List, Play } from "lucide-react";
import { useState } from "react";
import { games } from "../../catalogue/fixtures";
import type { Story } from "../../catalogue/stories";

/** Keyboard and pad focus (#161): the `focus.ring.*` tokens drawn by the
 * `focus-ring` utilities. Tab through the stories; Mode → Console draws the
 * edge inside each element with an inward glow and raises the scale, Effects
 * → Reduced drops the glows and keeps the edge, and each theme restyles it. */
const meta = { title: "Shell/Focus", parameters: { layout: "fullscreen" } } satisfies Meta;
export default meta;

const many = Array.from({ length: 10 }, (_, index) => {
  const game = games[index % games.length] as CardGame;
  return { ...game, title: `${game.title} ${Math.floor(index / games.length) + 1}` };
});

const frame = "overflow-hidden rounded-panel border border-dashed border-border-strong";

/** Each treatment drawn without focus, flush against a clipping frame, so
 * modes, themes and effects can be compared at a glance. */
export const Pinned: Story = {
  render: () => (
    <div className="flex flex-col gap-6 p-6">
      <div className={`${frame} flex w-fit gap-3`}>
        <span className="focus-ring inline-flex h-9 items-center rounded-control border border-border-strong bg-surface-overlay/70 px-4 text-sm">
          Button
        </span>
        <span className="focus-ring inline-flex h-9 items-center rounded-control bg-linear-to-r from-accent to-accent-strong px-4 text-sm text-accent-fg">
          Primary
        </span>
        <span className="focus-ring grid size-8 place-items-center rounded-control text-fg-muted">
          <Gamepad2 className="size-4" />
        </span>
        <span className="focus-ring grid size-4.5 place-items-center rounded-[5px] border border-border-strong bg-surface-base/60" />
      </div>
      <div className={`${frame} flex w-fit gap-3`}>
        <div className="focus-ring-over relative w-44 overflow-hidden rounded-card border border-border bg-surface-raised">
          <GameArt game={games[0] as CardGame} className="aspect-(--cs-card-aspect) w-full" />
          <p className="truncate px-3 py-2.5 text-sm font-semibold">Image card</p>
        </div>
        <div className="chamfer focus-ring-chamfer relative flex h-fit items-center gap-2 bg-linear-to-r from-accent to-accent-strong px-6 py-2.5 font-display font-semibold tracking-wider text-accent-fg uppercase">
          <Play className="size-4 fill-current" />
          Chamfer
        </div>
      </div>
      <div className={`${frame} w-64`}>
        {["First row", "Second row", "Last row"].map((label, index) => (
          <div
            key={label}
            className={`flex items-center px-3 py-2 text-sm ${index === 1 ? "focus-ring bg-surface-hover" : ""}`}
          >
            {label}
          </div>
        ))}
      </div>
    </div>
  ),
};

/** Focus at the first and last stop of scrolling and clipping parents, with
 * no padding to spare: a shelf of image cards and a list nested in a second
 * scroll region. */
export const ClippedEdges: Story = {
  render: () => (
    <div className="flex flex-col gap-6 p-6">
      <div className="grid grid-flow-col gap-4 overflow-x-auto">
        {many.map((game, index) => (
          <div key={game.title} className="w-(--cs-card-width)">
            <GameCard game={game} variant="recent" index={index} />
          </div>
        ))}
      </div>
      <div className="h-64 w-80 overflow-y-auto rounded-panel border border-border">
        {["Library", "Collections", "Store", "Activity"].map((label) => (
          <Button key={label} variant="ghost" className="w-full justify-start rounded-none">
            {label}
          </Button>
        ))}
        <div className="h-32 overflow-y-auto border-y border-border">
          {["Nested one", "Nested two", "Nested three", "Nested four"].map((label) => (
            <Button key={label} variant="secondary" className="w-full rounded-none">
              {label}
            </Button>
          ))}
        </div>
        {["Settings", "Quit"].map((label) => (
          <Button key={label} variant="ghost" className="w-full justify-start rounded-none">
            {label}
          </Button>
        ))}
      </div>
    </div>
  ),
};

function Controls() {
  const [layout, setLayout] = useState<"grid" | "list">("grid");
  const [sort, setSort] = useState<"recent" | "title">("recent");
  return (
    <div className="max-w-3xl p-6">
      <SettingsSection icon={Gamepad2} title="Small controls" description="Tab through each.">
        <SettingRow label="Layout">
          <SegmentedControl
            options={[
              { id: "grid", label: "Grid", icon: LayoutGrid },
              { id: "list", label: "List", icon: List },
            ]}
            value={layout}
            onChange={setLayout}
          />
        </SettingRow>
        <SettingRow label="Sort by">
          <Select
            options={[
              { id: "recent", label: "Recently played" },
              { id: "title", label: "Title" },
            ]}
            value={sort}
            onChange={setSort}
          />
        </SettingRow>
        <SettingRow label="Volume">
          <Slider defaultValue={40} />
        </SettingRow>
        <SettingRow label="Reduce effects">
          <Switch />
        </SettingRow>
        <SettingRow label="Scan subfolders">
          <Checkbox defaultSelected>Recursive</Checkbox>
        </SettingRow>
        <SettingRow label="ROM folder">
          <TextField
            icon={FolderOpen}
            defaultValue="/games/roms"
            action={
              <Button variant="secondary" size="sm">
                Browse…
              </Button>
            }
          />
        </SettingRow>
      </SettingsSection>
      <div className="mt-4 flex gap-3">
        <Button variant="primary" size="sm">
          Save
        </Button>
        <Button variant="danger" size="sm">
          Remove
        </Button>
        <Button variant="ghost" size="sm" isDisabled>
          Disabled
        </Button>
      </div>
    </div>
  );
}

export const SmallControls: Story = { render: () => <Controls /> };
