/** What syncs between paired devices (#227, `core.sync.categories`): each
 * kind of data syncs or stays on this device, and some never leave it.
 * This device's choice governs both what it sends and what it takes in;
 * turning one back on syncs it again from the change log. */
import { SegmentedControl, SettingRow, SettingsSection } from "@caskade/ui";
import type { MessageDescriptor } from "@lingui/core";
import { msg } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { HardDrive, RefreshCcw } from "lucide-react";
import { useSettingsSection } from "../useSettings";
import { useDeviceAdmin } from "./admin";

export type SyncCategory =
  | "library"
  | "artwork"
  | "achievementLinks"
  | "playSessions"
  | "saves"
  | "accounts"
  | "appearance"
  | "rommServers"
  | "launchActions"
  | "gameScripts"
  | "preferredEmulators"
  | "addons";

export type CategoryState = "sync" | "thisDeviceOnly";

/** `core.sync`, as the settings API holds it. */
export interface SyncSettingsValue {
  categories: Partial<Record<SyncCategory, CategoryState>>;
  neverRelay: boolean;
  port: number | null;
  /** Copies other devices send here start without asking (#235). */
  acceptCopies?: boolean;
}

/** Synced by default, in the order Settings lists them. */
const synced: SyncCategory[] = [
  "library",
  "playSessions",
  "accounts",
  "artwork",
  "saves",
  "achievementLinks",
  "rommServers",
  "appearance",
];

/** Kept on each device until the user turns them on. */
const optional: SyncCategory[] = ["launchActions", "gameScripts", "preferredEmulators", "addons"];

const labels: Record<SyncCategory, [MessageDescriptor, MessageDescriptor]> = {
  library: [
    msg`Library`,
    msg`Game details you changed, favourites, hidden games, collections, matches you set and games you kept apart.`,
  ],
  playSessions: [
    msg`Play history`,
    msg`Every session from every device, so play time adds up wherever you play.`,
  ],
  accounts: [
    msg`Accounts and PINs`,
    msg`Who plays here, their age limits and PINs. Wrong PIN attempts are counted on each device.`,
  ],
  artwork: [msg`Your artwork`, msg`Covers and images you chose or imported.`],
  saves: [msg`Saves`, msg`Game saves, with every version kept.`],
  achievementLinks: [
    msg`Achievement links you made`,
    msg`Games you linked to their achievements by hand.`,
  ],
  rommServers: [
    msg`RomM servers`,
    msg`Servers and what they import. Each device signs in with its own password.`,
  ],
  appearance: [msg`Appearance`, msg`Theme, accent and layout.`],
  launchActions: [
    msg`Launch actions`,
    msg`Play and utility actions you added. Their programs must be at the same place on each device.`,
  ],
  gameScripts: [
    msg`Game scripts`,
    msg`Scripts that run with one game. They arrive turned off on other devices.`,
  ],
  preferredEmulators: [
    msg`Preferred emulators`,
    msg`Which emulator each platform uses, where the other device has it.`,
  ],
  addons: [
    msg`Plugins and themes`,
    msg`Offers to install the same ones. Plugins still ask for their permissions on each device.`,
  ],
};

const localOnly: MessageDescriptor[] = [
  msg`Where games are installed`,
  msg`Storage folders`,
  msg`Emulators and their profiles`,
  msg`ROM folders`,
  msg`Store sign-ins (connect each device)`,
  msg`Downloaded metadata and images`,
  msg`Console mode, window and start-up choices`,
];

/** A category's state when the settings don't name it: synced ones sync. */
function stateOf(value: SyncSettingsValue, category: SyncCategory): CategoryState {
  return value.categories[category] ?? (synced.includes(category) ? "sync" : "thisDeviceOnly");
}

export function SyncChoices() {
  const { t } = useLingui();
  const admin = useDeviceAdmin();
  const { settings, setSettings, saving } = useSettingsSection<SyncSettingsValue>("core.sync");
  if (!settings) return null;
  const options = [
    { id: "sync", label: t({ message: "Sync", context: "sync choice" }) },
    { id: "thisDeviceOnly", label: t`This device only` },
  ] as const;
  const row = (category: SyncCategory) => {
    const [label, description] = labels[category];
    return (
      <SettingRow key={category} label={t(label)} description={t(description)}>
        <SegmentedControl<CategoryState>
          options={options}
          value={stateOf(settings, category)}
          isDisabled={!admin || saving}
          onChange={(state) =>
            setSettings((current) => ({
              ...current,
              categories: { ...current.categories, [category]: state },
            }))
          }
        />
      </SettingRow>
    );
  };
  return (
    <>
      <SettingsSection
        icon={RefreshCcw}
        title={t`What syncs`}
        description={t`Changes made on any device reach the others when they next connect. Where two devices changed the same thing, the later change wins. This device only stops this device sending and taking it in.`}
      >
        <div className="flex flex-col">{synced.map(row)}</div>
        <h3 className="mt-6 text-sm font-medium">
          <Trans>Usually different on each device</Trans>
        </h3>
        <p className="mb-1 text-xs text-fg-subtle">
          <Trans>These stay on each device until you choose to sync them.</Trans>
        </p>
        <div className="flex flex-col">{optional.map(row)}</div>
      </SettingsSection>
      <SettingsSection
        icon={HardDrive}
        title={t`Never synced`}
        description={t`These belong to each device's own drives and hardware.`}
      >
        <ul className="grid grid-cols-[repeat(auto-fill,minmax(16rem,1fr))] gap-x-6 gap-y-1.5 text-sm text-fg-muted">
          {localOnly.map((label) => (
            <li key={label.id} className="flex items-center gap-2">
              <span aria-hidden className="size-1 rounded-full bg-fg-subtle" />
              {t(label)}
            </li>
          ))}
        </ul>
      </SettingsSection>
    </>
  );
}
