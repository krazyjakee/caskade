/** The browser preview's in-memory backends. `main.tsx` loads this only in a
 * dev build outside Tauri, so production builds drop it. Backup, Streaming,
 * Store accounts, Shares and Devices always get theirs; games list ports, and
 * Add-ons, Achievements, the Store catalog and the plugin manager get theirs,
 * only with `full` (`VITE_CASKADE_FIXTURES=1`). */
import { previewAchievements } from "./achievements";
import { createAchievementsFixture } from "./achievementsFixtures";
import { previewAddons } from "./addons";
import { createAddonsFixture } from "./addonsFixtures";
import { previewBackup } from "./backup";
import { createBackupFixtureApi } from "./backupFixtures";
import { createCatalogFixtureApi } from "./catalogFixtures";
import { previewCatalog } from "./igdb";
import { previewPluginManager } from "./pluginManager";
import { createPluginManagerFixture } from "./pluginManagerFixtures";
import { previewPorts } from "./ports";
import { createPortsFixtureApi } from "./portsFixtures";
import { previewShares } from "./shares";
import { createSharesFixtureApi } from "./sharesFixtures";
import { previewStoreAccounts } from "./storeAccounts";
import { createStoreAccountsFixtureApi } from "./storeAccountsFixtures";
import { previewStreaming } from "./streaming";
import { createStreamingFixtureApi } from "./streamingFixtures";
import { previewSync } from "./sync";
import { createSyncFixtureApi } from "./syncFixtures";

export function previewFixtures(full: boolean) {
  previewBackup(createBackupFixtureApi());
  previewShares(createSharesFixtureApi());
  previewStoreAccounts(createStoreAccountsFixtureApi());
  previewStreaming(createStreamingFixtureApi());
  previewSync(createSyncFixtureApi());
  previewPorts(createPortsFixtureApi(full ? undefined : []));
  if (!full) return;
  previewAddons(createAddonsFixture());
  previewAchievements(createAchievementsFixture());
  previewCatalog(createCatalogFixtureApi());
  previewPluginManager(createPluginManagerFixture());
}
