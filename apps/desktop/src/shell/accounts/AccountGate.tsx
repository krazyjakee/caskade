import { useLingui } from "@lingui/react/macro";
import { useRouter } from "@tanstack/react-router";
import { type ReactNode, useRef } from "react";
import { useAccounts, useAccountsApi } from "../../api/accounts";
import { FailureNotice } from "../../components/FailureNotice";
import { useDefaultFocus } from "../../input";
import { useAppWindow } from "../appWindow";
import { useMode } from "../console/mode";
import { useWindowDrag } from "../useWindowDrag";
import { WindowControls } from "../WindowControls";
import { AccountPicker } from "./AccountPicker";

/** The account picker in place of the shell while nobody is signed in
 * (#207). The core signs a single account without a PIN in at start, so a
 * library that never set accounts up opens as before. Nothing shows while
 * the accounts load, so a locked library never flashes up; without
 * accounts (the browser preview) the shell shows. */
export function AccountGate({ children }: { children: ReactNode }) {
  const api = useAccountsApi();
  const { data, isPending, isError, error, refetch } = useAccounts();
  const router = useRouter();
  if (api && isPending && !isError) return null;
  // Not knowing who is signed in must not open the shell, where every
  // change would be refused (#160): say why, and offer to ask again.
  if (api && isError && !data)
    return (
      <AccountsUnavailable
        error={error}
        retry={() => {
          void refetch();
        }}
      />
    );
  if (!api || !data || data.accounts.length === 0) return children;
  if (data.accounts.some((account) => account.id === data.activeId)) return children;
  return (
    <AccountPicker
      list={data}
      onSignedIn={() => {
        // A collection page belonged to whoever was signed in before.
        if (router.state.location.pathname.startsWith("/collections/"))
          void router.navigate({ to: "/collections", search: { filter: "all" } });
      }}
    />
  );
}

function AccountsUnavailable({ error, retry }: { error: unknown; retry: () => void }) {
  const { t } = useLingui();
  const appWindow = useAppWindow();
  useWindowDrag(appWindow);
  const consoleMode = useMode().mode === "console";
  const button = useRef<HTMLButtonElement>(null);
  useDefaultFocus(button);
  return (
    <div className="flex h-full flex-col">
      <header data-window-drag className="flex h-topbar shrink-0 items-center justify-end pr-4">
        {appWindow?.frame === "custom" && !consoleMode && <WindowControls window={appWindow} />}
      </header>
      <main data-focus-section="main" className="grid flex-1 place-items-center px-6">
        <FailureNotice
          className="w-full max-w-lg"
          error={error}
          title={t`Caskade couldn't check who is signed in`}
          onRetry={retry}
          retryRef={button}
        />
      </main>
    </div>
  );
}
