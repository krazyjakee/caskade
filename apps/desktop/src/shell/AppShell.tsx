import { useLingui } from "@lingui/react/macro";
import { Outlet, useRouter, useRouterState } from "@tanstack/react-router";
import { GameMenuHost } from "../components/gameMenu/GameMenuHost";
import { PathPickerProvider } from "../components/pathBrowser/PathPicker";
import { PairingHost } from "../components/sync/PairingHost";
import { InputProvider, useInputAction } from "../input";
import { GuideMenu, nativeGuideApi } from "../input/GuideMenu";
import { useOptionalInput } from "../input/InputProvider";
import { PluginNotices } from "../plugins/PluginViews";
import { JobQuestionHost } from "../views/activity/JobQuestionHost";
import { useOpenGame } from "../views/game/origin";
import { AccountGate } from "./accounts/AccountGate";
import { ConsoleShell } from "./console/ConsoleShell";
import { GameCurtainProvider, useGameCurtain } from "./console/GameCurtain";
import { useMode } from "./console/mode";
import { Sidebar } from "./Sidebar";
import { StatusBar } from "./StatusBar";
import { Topbar } from "./Topbar";
import { TypeSearchProvider, useTypeSearch } from "./TypeSearch";

/** Actions that work everywhere: View (Select) switches between the shells
 * (#148), Y goes to the search (#150): the search box and the on-screen
 * keyboard for a pad, the search overlay for keys. */
function ShellActions() {
  const { t } = useLingui();
  const { mode, toggle } = useMode();
  const input = useOptionalInput();
  const typeSearch = useTypeSearch();
  useInputAction("view", toggle, {
    label: mode === "console" ? t`Desktop mode` : t`Console mode`,
  });
  useInputAction(
    "search",
    () => {
      if (typeSearch && !input?.typesOnScreen()) {
        typeSearch.open("");
        return true;
      }
      const search = document.querySelector<HTMLInputElement>('[data-cs="search"] input');
      if (!search) return false;
      search.focus();
      search.select();
      // A pad types on the on-screen keyboard.
      input?.keyboard(search);
      return true;
    },
    { label: t({ message: "Search", context: "action" }) },
  );
  return null;
}

/** The guide's quick menu; back to the game brings the curtain down again. */
function Guide() {
  const openGame = useOpenGame();
  const curtain = useGameCurtain();
  return (
    <GuideMenu
      api={{
        ...nativeGuideApi,
        returnToGame: async () => {
          curtain.cover();
          await nativeGuideApi.returnToGame();
        },
      }}
      openDetails={openGame}
    />
  );
}

export function AppShell() {
  // Keyed by path so each view replays its enter animation on navigation.
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  // Pads and the console keyboard map (#147); focus comes back per history entry.
  const locationKey = useRouterState({
    select: (s) => s.location.state.__TSR_key ?? s.location.href,
  });
  const router = useRouter();
  // Console mode (#148) swaps the shell; the routes and views stay the same.
  const consoleMode = useMode().mode === "console";
  return (
    <InputProvider
      console={consoleMode}
      locationKey={locationKey}
      onBack={() => router.history.back()}
    >
      {/* Nobody signed in: the account picker instead (#207). */}
      <AccountGate>
        <GameCurtainProvider active={consoleMode}>
          {/* File and folder pickers: in console mode, the in-app browser. */}
          <PathPickerProvider>
            <TypeSearchProvider>
              <ShellActions />
              <JobQuestionHost />
              <PairingHost />
              <GameMenuHost>
                {consoleMode ? (
                  <ConsoleShell pathname={pathname} />
                ) : (
                  <div data-cs="app-shell" className="relative flex h-full flex-col overflow-clip">
                    <div data-cs="backdrop" aria-hidden />
                    <div className="flex min-h-0 flex-1">
                      <Sidebar />
                      <div data-part="main" className="relative flex min-w-0 flex-1 flex-col">
                        <Topbar />
                        <main
                          data-cs="view"
                          data-focus-section="main"
                          className="min-h-0 flex-1 overflow-y-auto px-6 pb-8"
                        >
                          <div key={pathname} className="flex min-h-full animate-enter flex-col">
                            <Outlet />
                          </div>
                        </main>
                      </div>
                    </div>
                    <StatusBar />
                  </div>
                )}
              </GameMenuHost>
            </TypeSearchProvider>
          </PathPickerProvider>
          <PluginNotices />
          <Guide />
        </GameCurtainProvider>
      </AccountGate>
    </InputProvider>
  );
}
