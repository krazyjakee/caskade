import { msg } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { useNavigate } from "@tanstack/react-router";
import {
  Activity,
  DatabaseBackup,
  Gamepad2,
  HardDrive,
  Info,
  PackageOpen,
  Palette,
  Puzzle,
  Rocket,
  ScanSearch,
  Settings2,
  Store,
  TerminalSquare,
  UserRound,
  Users,
} from "lucide-react";
import { type ReactNode, useRef } from "react";
import { Tab, TabList, TabPanel, Tabs } from "react-aria-components";
import { useActiveAccount } from "../api/accounts";
import { useScreenTabs } from "../input";
import { AppearanceSettings } from "./settings/AppearanceSettings";
import { AboutSettings } from "./settings/about/AboutSettings";
import { AccountsSettings } from "./settings/accounts/AccountsSettings";
import { ProfileSettings } from "./settings/accounts/ProfileSettings";
import { BackupSettings } from "./settings/backup/BackupSettings";
import { EmulatorSettings } from "./settings/EmulatorSettings";
import { GeneralSettings } from "./settings/GeneralSettings";
import { InstallerSettings } from "./settings/InstallerSettings";
import { LaunchersSettings } from "./settings/LaunchersSettings";
import { LibraryDiscoverySettings } from "./settings/LibraryDiscoverySettings";
import { PluginsSettings } from "./settings/PluginsSettings";
import { ScriptsSettings } from "./settings/ScriptsSettings";
import { StorageSettings } from "./settings/StorageSettings";
import { StatisticsSettings } from "./settings/statistics/StatisticsSettings";
import { StoreAccountsSettings } from "./settings/stores/StoreAccountsSettings";
import { TorrentLinksSettings } from "./settings/TorrentLinksSettings";
import { UpdateSettings } from "./settings/UpdateSettings";
import { SettingsProvider } from "./settings/useSettings";

/** Labels are translated where they're shown (`t(tab.label)`). */
export const settingsTabs = [
  { id: "general", label: msg({ message: "General", context: "settings tab" }), icon: Settings2 },
  {
    id: "library-discovery",
    label: msg({ message: "Library Discovery", context: "settings tab" }),
    icon: ScanSearch,
  },
  { id: "launchers", label: msg({ message: "Launchers", context: "settings tab" }), icon: Rocket },
  // Admins only (#180): global scripts run with every game.
  {
    id: "scripts",
    label: msg({ message: "Scripts", context: "settings tab" }),
    icon: TerminalSquare,
  },
  // The id stays `emulators`: it is in saved selections and links.
  {
    id: "emulators",
    label: msg({ message: "Emulation", context: "settings tab" }),
    icon: Gamepad2,
  },
  {
    id: "appearance",
    label: msg({ message: "Appearance", context: "settings tab" }),
    icon: Palette,
  },
  { id: "storage", label: msg({ message: "Storage", context: "settings tab" }), icon: HardDrive },
  {
    id: "installer",
    label: msg({ message: "Installer", context: "settings tab" }),
    icon: PackageOpen,
  },
  { id: "plugins", label: msg({ message: "Plugins", context: "settings tab" }), icon: Puzzle },
  // Admins only (#165): stores' sign-ins.
  {
    id: "stores",
    label: msg({ message: "Store accounts", context: "settings tab" }),
    icon: Store,
  },
  // Admins only (#193): a backup holds every account's data.
  {
    id: "backup",
    label: msg({ message: "Backup", context: "settings tab" }),
    icon: DatabaseBackup,
  },
  // Admins only (#208): standard accounts are never given it.
  { id: "accounts", label: msg({ message: "Accounts", context: "settings tab" }), icon: Users },
  {
    id: "profile",
    label: msg({ message: "My profile", context: "settings tab" }),
    icon: UserRound,
  },
  {
    id: "statistics",
    label: msg({ message: "Statistics", context: "settings tab" }),
    icon: Activity,
  },
  { id: "about", label: msg({ message: "About", context: "settings tab" }), icon: Info },
] as const;
export type SettingsTab = (typeof settingsTabs)[number]["id"];

const panels: Record<SettingsTab, () => ReactNode> = {
  general: GeneralSettings,
  "library-discovery": LibraryDiscoverySettings,
  launchers: LaunchersSettings,
  scripts: ScriptsSettings,
  emulators: EmulatorSettings,
  appearance: AppearanceSettings,
  storage: StorageSettings,
  installer: () => (
    <>
      <InstallerSettings />
      <TorrentLinksSettings />
      <UpdateSettings />
    </>
  ),
  plugins: PluginsSettings,
  stores: StoreAccountsSettings,
  backup: BackupSettings,
  accounts: AccountsSettings,
  profile: ProfileSettings,
  statistics: StatisticsSettings,
  about: AboutSettings,
};

export function SettingsView({ tab }: { tab: SettingsTab }) {
  return (
    <SettingsProvider>
      <SettingsContent tab={tab} />
    </SettingsProvider>
  );
}
/** Whether the signed-in account may change a section (#205): an admin
 * every one, a standard account those it was given. The core refuses the
 * rest whatever this shows. About is for everyone; without accounts (the
 * browser preview) every section is shown. */
function useEditable() {
  const account = useActiveAccount();
  return (id: SettingsTab) =>
    id === "about" ||
    // Statistics only reads the machine (#156).
    id === "statistics" ||
    // Every account changes its own name, avatar and PIN (#208).
    id === "profile" ||
    account === undefined ||
    account?.role === "admin" ||
    (account?.permissions.settings.includes(id) ?? false);
}

function Forbidden() {
  return (
    <p className="text-fg-muted">
      <Trans>Your account can't change these settings. Ask an admin.</Trans>
    </p>
  );
}

function SettingsContent({ tab }: { tab: SettingsTab }) {
  const { t } = useLingui();
  const navigate = useNavigate();
  const tablist = useRef<HTMLDivElement>(null);
  const editable = useEditable();
  const tabs = settingsTabs.filter(({ id }) => editable(id));
  const go = (next: SettingsTab) =>
    void navigate({ to: "/settings", search: { tab: next }, replace: true });
  // LB/RB move between the sections (#151), in place: Back leaves Settings.
  useScreenTabs(
    tabs.map((section) => section.id),
    tab,
    go,
    { tabs: tablist },
  );
  const Panel = editable(tab) ? panels[tab] : Forbidden;
  return (
    <div data-view="settings">
      <h1 className="font-display text-3xl font-semibold tracking-wide">
        <Trans>Settings</Trans>
      </h1>
      <p className="mt-1 text-fg-muted">
        <Trans>Configure library discovery, launchers, emulation and system integration.</Trans>
      </p>
      <Tabs
        orientation="vertical"
        selectedKey={tab}
        onSelectionChange={(key) => go(key as SettingsTab)}
        className="mt-6 flex items-start gap-4 sm:gap-6"
      >
        {/* Sticky beside a long panel; scrolls on its own once the sections
            outgrow the window, and focus scrolls it along. */}
        <TabList
          ref={tablist}
          aria-label={t`Settings sections`}
          data-cs="settings-nav"
          // Its own focus section: a pad going left out of the panel comes
          // back to the section it left from.
          data-focus-section="settings-nav"
          className="sticky top-0 flex max-h-[calc(100dvh-12rem)] w-40 shrink-0 flex-col gap-1 overflow-x-hidden overflow-y-auto overscroll-contain p-2 sm:w-48 lg:w-56"
        >
          {tabs.map(({ id, label, icon: Icon }) => (
            <Tab
              key={id}
              id={id}
              data-part="tab"
              // A pad starts on the open section.
              data-focus-default={id === tab || undefined}
              className="flex cursor-pointer items-center gap-2 rounded-control border border-transparent px-3 py-2 text-sm text-fg-muted transition-colors duration-(--cs-motion-duration-fast) data-hovered:bg-surface-raised data-hovered:text-fg data-selected:border-accent-alt/50 data-selected:bg-accent/20 data-selected:text-fg data-selected:shadow-[0_0_16px_-6px] data-selected:shadow-accent-alt"
            >
              <Icon data-part="icon" aria-hidden className="size-4 shrink-0" />
              <span data-part="label" className="min-w-0 break-words">
                {t(label)}
              </span>
            </Tab>
          ))}
        </TabList>
        {/* Not a stop of its own for a pad while a panel is loading. */}
        <TabPanel id={tab} key={tab} data-spatial="skip" className="min-w-0 flex-1 animate-enter">
          <Panel />
        </TabPanel>
      </Tabs>
    </div>
  );
}
