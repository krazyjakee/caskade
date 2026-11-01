import {
  Button,
  Checkbox,
  Pill,
  SegmentedControl,
  Select,
  SettingRow,
  SettingsSection,
  Slider,
  StatusDot,
  Switch,
  TextField,
} from "@caskade/ui";
import type { Meta } from "@storybook/react-vite";
import {
  ChartColumn,
  FolderOpen,
  Gamepad2,
  Gauge,
  HardDrive,
  LayoutGrid,
  List,
  PackageOpen,
  Palette,
  Puzzle,
  Rocket,
  ScanSearch,
  Settings2,
  Sparkles,
} from "lucide-react";
import { useState } from "react";
import { Tab, TabList, TabPanel, Tabs } from "react-aria-components";
import type { Story } from "../../catalogue/stories";

/** Settings rows as the Settings panels lay them out: every control inside a
 * `setting-row`, labelled by the row. */
const meta = { title: "Shell/Settings" } satisfies Meta;
export default meta;

function Appearance() {
  const [layout, setLayout] = useState<"grid" | "list">("grid");
  const [sort, setSort] = useState<"recent" | "title">("recent");
  return (
    <div className="flex max-w-3xl flex-col gap-6">
      <SettingsSection
        icon={Palette}
        title="Appearance"
        description="How Caskade looks."
        aside={
          <Pill tone="accent" icon={Sparkles}>
            New
          </Pill>
        }
      >
        <SettingRow icon={LayoutGrid} label="Library layout" description="Cards or rows.">
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
        <SettingRow icon={Gauge} label="UI scale" description="Text and controls.">
          <Slider defaultValue={100} minValue={80} maxValue={150} format={(v) => `${v}%`} />
        </SettingRow>
        <SettingRow label="Reduce effects" description="No blur, glow or scanlines.">
          <Switch />
        </SettingRow>
      </SettingsSection>
      <SettingsSection icon={Gamepad2} title="Emulation" description="Where your emulators live.">
        <SettingRow label="RetroArch" description="Multi-system">
          <StatusDot tone="ok">Detected</StatusDot>
          <Switch defaultSelected />
        </SettingRow>
        <SettingRow label="Dolphin" description="GameCube · Wii">
          <StatusDot tone="off">Not found</StatusDot>
        </SettingRow>
        <SettingRow label="ROM folder">
          <TextField
            icon={FolderOpen}
            defaultValue="/games/roms"
            action={<Button variant="secondary">Browse…</Button>}
          />
        </SettingRow>
        <SettingRow label="Scan subfolders">
          <Checkbox defaultSelected>Recursive</Checkbox>
        </SettingRow>
      </SettingsSection>
    </div>
  );
}

export const SettingsRows: Story = { render: () => <Appearance /> };

/** Settings' sections, as `SettingsView` lays them out (#155): a vertical
 * tab list beside the open panel. Two more sections than the app has, one
 * with a long name, in a short frame, so the list wraps and scrolls. */
const sections = [
  { id: "general", label: "General", icon: Settings2 },
  { id: "library-discovery", label: "Library Discovery", icon: ScanSearch },
  { id: "launchers", label: "Launchers", icon: Rocket },
  { id: "emulators", label: "Emulation", icon: Gamepad2 },
  { id: "appearance", label: "Appearance", icon: Palette },
  { id: "storage", label: "Storage", icon: HardDrive },
  { id: "installer", label: "Installer", icon: PackageOpen },
  { id: "plugins", label: "Plugins", icon: Puzzle },
  { id: "statistics", label: "Statistics", icon: ChartColumn },
  { id: "long", label: "Accessibility and controller layouts", icon: Gamepad2 },
];

function SectionList() {
  const [open, setOpen] = useState<string>("emulators");
  return (
    <div className="h-96 overflow-y-auto p-6">
      <Tabs
        orientation="vertical"
        selectedKey={open}
        onSelectionChange={(key) => setOpen(String(key))}
        className="flex items-start gap-6"
      >
        <TabList
          aria-label="Settings sections"
          className="sticky top-0 flex max-h-80 w-48 shrink-0 flex-col gap-1 overflow-x-hidden overflow-y-auto overscroll-contain p-2"
        >
          {sections.map(({ id, label, icon: Icon }) => (
            <Tab
              key={id}
              id={id}
              className="flex cursor-pointer items-center gap-2 rounded-control border border-transparent px-3 py-2 text-sm text-fg-muted transition-colors duration-(--cs-motion-duration-fast) data-hovered:bg-surface-raised data-hovered:text-fg data-selected:border-accent-alt/50 data-selected:bg-accent/20 data-selected:text-fg data-selected:shadow-[0_0_16px_-6px] data-selected:shadow-accent-alt"
            >
              <Icon aria-hidden className="size-4 shrink-0" />
              <span className="min-w-0 break-words">{label}</span>
            </Tab>
          ))}
        </TabList>
        <TabPanel id={open} className="min-w-0 flex-1">
          <Appearance />
        </TabPanel>
      </Tabs>
    </div>
  );
}

export const SettingsSections: Story = { render: () => <SectionList /> };
