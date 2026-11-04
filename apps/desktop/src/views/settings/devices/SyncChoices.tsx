/** What syncs between paired devices (#227, `core.sync.categories`): each
 * kind of data syncs or stays on this device, and some never leave it.
 * This device's choice governs both what it sends and what it takes in;
 * turning one back on syncs it again from the change log. Before any device
 * is paired it all waits behind a disclosure (#60). */
import { Button, SegmentedControl, SettingRow, SettingsSection } from "@caskade/ui";
import type { MessageDescriptor } from "@lingui/core";
import { msg } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { ChevronDown, ChevronRight, HardDrive, RefreshCcw } from "lucide-react";
import { type ReactNode, useLayoutEffect, useRef, useState } from "react";
import { Disclosure, DisclosurePanel, Heading } from "react-aria-components";
import { flushSync } from "react-dom";
import { SettingsStatus, useSettingsSection } from "../useSettings";
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
  | "addons"
  | "emulatorConfig";

export type CategoryState = "sync" | "thisDeviceOnly";

/** `core.sync`, as the settings API holds it. */
export interface SyncSettingsValue {
  categories: Partial<Record<SyncCategory, CategoryState>>;
  neverRelay: boolean;
  port: number | null;
  /** Downloads come from a device nearby that has them first (#236). */
  peerDownloads?: boolean;
  /** Copies other devices send here start without asking (#235). */
  acceptCopies?: boolean;
  /** Other devices can start games here (#234). */
  remotePlay?: boolean;
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
const optional: SyncCategory[] = [
  "launchActions",
  "gameScripts",
  "preferredEmulators",
  "emulatorConfig",
  "addons",
];

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
    msg`Which emulator each platform uses. One this device doesn't have is offered in Settings → Emulation.`,
  ],
  emulatorConfig: [
    msg`Emulator configuration`,
    msg`Controller mappings, core options and shader presets, each in this device's own emulator folders.`,
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

/** The choices behind a button. Settings search marks the row it finds
 * `data-found` but can't reach it while it's hidden, so this opens up and
 * finishes the job. Watching from the layout effect: search looks as soon as
 * the panel shows the row, before a plain effect would run. */
function ChooseAheadOfTime({ label, children }: { label: string; children: ReactNode }) {
  const [expanded, setExpanded] = useState(false);
  const panel = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const root = panel.current;
    if (!root) return;
    const observer = new MutationObserver((records) => {
      const found = records
        .map((record) => record.target as HTMLElement)
        .find((target) => target.hasAttribute("data-found"));
      if (!found) return;
      flushSync(() => setExpanded(true));
      found.querySelector("input")?.focus();
      found.scrollIntoView?.({ block: "center" });
    });
    observer.observe(root, { subtree: true, attributes: true, attributeFilter: ["data-found"] });
    return () => observer.disconnect();
  }, []);
  return (
    <Disclosure isExpanded={expanded} onExpandedChange={setExpanded}>
      <Heading>
        <Button
          slot="trigger"
          variant="ghost"
          size="sm"
          icon={expanded ? ChevronDown : ChevronRight}
        >
          {label}
        </Button>
      </Heading>
      <DisclosurePanel ref={panel}>{children}</DisclosurePanel>
    </Disclosure>
  );
}

export function SyncChoices({ collapsed = false }: { collapsed?: boolean }) {
  const { t } = useLingui();
  const admin = useDeviceAdmin();
  const sync = useSettingsSection<SyncSettingsValue>("core.sync");
  const { settings, setSettings, saving } = sync;
  const title = t`What syncs`;
  const description = t`Changes made on any device reach the others when they next connect. Where two devices changed the same thing, the later change wins. This device only keeps a kind out of sync here: this device neither sends its changes nor takes the others'.`;
  if (!settings) {
    // Loading says nothing; a failed load says why, with Try again.
    return sync.loadError ? (
      <SettingsSection icon={RefreshCcw} title={title} description={description}>
        <SettingsStatus section={sync} what={title} className="" />
      </SettingsSection>
    ) : null;
  }
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
  const rows = (
    <>
      <div className="flex flex-col">{synced.map(row)}</div>
      <h3 className="mt-6 text-sm font-medium">
        <Trans>Usually different on each device</Trans>
      </h3>
      <p className="mb-1 text-xs text-fg-muted">
        <Trans>These stay on each device until you choose to sync them.</Trans>
      </p>
      <div className="flex flex-col">{optional.map(row)}</div>
    </>
  );
  const neverTitle = t`Never synced`;
  const neverDescription = t`These belong to each device's own drives and hardware.`;
  const never = (
    <ul className="grid grid-cols-[repeat(auto-fill,minmax(16rem,1fr))] gap-x-6 gap-y-1.5 text-sm text-fg-muted">
      {localOnly.map((label) => (
        <li key={label.id} className="flex items-center gap-2">
          <span aria-hidden className="size-1 rounded-full bg-fg-subtle" />
          {t(label)}
        </li>
      ))}
    </ul>
  );
  if (collapsed) {
    return (
      <SettingsSection
        icon={RefreshCcw}
        title={title}
        description={t`Nothing syncs until a device is paired. Then your library, play history and saves stay in step, unless you choose otherwise.`}
      >
        <SettingsStatus section={sync} what={title} className="mb-3" />
        <ChooseAheadOfTime label={admin ? t`Choose what syncs` : t`See what syncs`}>
          <p className="mt-2 text-sm text-fg-muted">{description}</p>
          {rows}
          <h3 className="mt-6 text-sm font-medium">{neverTitle}</h3>
          <p className="mb-2 text-xs text-fg-muted">{neverDescription}</p>
          {never}
        </ChooseAheadOfTime>
      </SettingsSection>
    );
  }
  return (
    <>
      <SettingsSection icon={RefreshCcw} title={title} description={description}>
        <SettingsStatus section={sync} what={title} className="mb-3" />
        {rows}
      </SettingsSection>
      <SettingsSection icon={HardDrive} title={neverTitle} description={neverDescription}>
        {never}
      </SettingsSection>
    </>
  );
}
