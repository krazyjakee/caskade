import "@fontsource-variable/inter";
import "@fontsource-variable/jetbrains-mono";
import "@fontsource-variable/oxanium";
import { activateLocale, i18n, sourceLocale } from "@caskade/i18n";
import { I18nProvider } from "@lingui/react";
import { RouterProvider } from "@tanstack/react-router";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { connectLibraryApi } from "./api/bootstrap";
import { nativeDiscoveryApi } from "./api/discoveryNative";
import { connectEmulationApi } from "./api/emulationNative";
import { connectInstallApi } from "./api/installNative";
import { InstallRequestContext } from "./api/installRequests";
import { nativeInstallRequestApi } from "./api/installRequestsNative";
import { nativeLocaleApi } from "./api/locale";
import { createLibraryQueryClient, LibraryProvider } from "./api/provider";
import { StorageProvider } from "./api/StorageProvider";
import { nativeStorageApi } from "./api/storageNative";
import { AppearanceRoot } from "./appearance/AppearanceRoot";
import { startAppearance } from "./appearance/startup";
import { DropProvider } from "./drop/DropProvider";
import { applyLocale, LocaleProvider } from "./locale/LocaleProvider";
import { OnboardingGate } from "./onboarding/OnboardingGate";
import { connectPluginApi } from "./plugins/native";
import { PluginProvider } from "./plugins/PluginProvider";
import { router } from "./router";
import { applyMode, ModeProvider, nativeConsoleApi } from "./shell/console/mode";
import { AppErrorBoundary } from "./shell/ErrorFallback";
import { suppressNativeContextMenu } from "./shell/nativeMenu";
import { ActivityProvider } from "./views/activity/ActivityProvider";
import { InstallIngress } from "./views/activity/InstallIngress";
import { DiscoveryProvider } from "./views/settings/discovery/DiscoveryProvider";
import { EmulationProvider } from "./views/settings/emulators/EmulationProvider";
import { SettingsProvider } from "./views/settings/useSettings";
import "./styles/index.css";

const root = document.getElementById("root");
if (!root) throw new Error("#root element missing from index.html");
suppressNativeContextMenu();

const api = await connectLibraryApi();
const installApi = await connectInstallApi();
const emulationApi = await connectEmulationApi();
const queryClient = createLibraryQueryClient();
const pluginApi = connectPluginApi();
// The chosen theme and user CSS apply before the first render, so the
// default look never flashes in (#139, #140, #142).
await startAppearance();
// Console mode (#148) is known before the first render too.
const initialMode = await nativeConsoleApi.initial().catch(() => "desktop" as const);
applyMode(initialMode);
// The core's language (#213) is known before the first render too. The
// browser preview follows the browser's, or `?lang=sv` / `?lang=en-XA`.
const initialLocale = await nativeLocaleApi.current().catch((error: unknown) => {
  console.warn("language unavailable", error);
  return { locale: sourceLocale, system: sourceLocale, available: [sourceLocale] };
});
await activateLocale(initialLocale.locale).catch(() => activateLocale(sourceLocale));
applyLocale(i18n.locale);
createRoot(root).render(
  <StrictMode>
    <I18nProvider i18n={i18n}>
      <LocaleProvider api={nativeLocaleApi} initial={{ ...initialLocale, locale: i18n.locale }}>
        <AppErrorBoundary>
          <DropProvider>
            <InstallRequestContext.Provider value={nativeInstallRequestApi}>
              <LibraryProvider api={api} client={queryClient}>
                <StorageProvider api={nativeStorageApi}>
                  <ActivityProvider api={installApi} storageApi={nativeStorageApi}>
                    <DiscoveryProvider api={nativeDiscoveryApi}>
                      <EmulationProvider api={emulationApi}>
                        <AppearanceRoot />
                        <ModeProvider initial={initialMode}>
                          <PluginProvider api={pluginApi} library={api}>
                            {/* First-run setup comes before the app; finishing it
                          opens the library. */}
                            <SettingsProvider>
                              <OnboardingGate
                                onDone={() =>
                                  void router.navigate({
                                    to: "/library",
                                    search: { filter: "all" },
                                  })
                                }
                              >
                                <RouterProvider router={router} />
                              </OnboardingGate>
                            </SettingsProvider>
                          </PluginProvider>
                        </ModeProvider>
                        <InstallIngress api={nativeInstallRequestApi} />
                      </EmulationProvider>
                    </DiscoveryProvider>
                  </ActivityProvider>
                </StorageProvider>
              </LibraryProvider>
            </InstallRequestContext.Provider>
          </DropProvider>
        </AppErrorBoundary>
      </LocaleProvider>
    </I18nProvider>
  </StrictMode>,
);
