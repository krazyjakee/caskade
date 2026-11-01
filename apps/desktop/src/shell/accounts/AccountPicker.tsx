/** Who is playing (#207): the accounts to sign in as, shown in place of the
 * shell at start (when there is more than one account, or the only one has
 * a PIN) and after Switch account. A locked account asks for its PIN on the
 * PIN pad. A controller starts on the account that signed in last. */
import { Avatar } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { ArrowLeft, Lock } from "lucide-react";
import { type CSSProperties, useRef, useState } from "react";
import { Button } from "react-aria-components";
import {
  type Account,
  type AccountList,
  lastAccountId,
  rememberAccount,
  useSignIn,
} from "../../api/accounts";
import { PinPad } from "../../components/accounts/PinPad";
import { useDefaultFocus, useInputAction } from "../../input";
import { useAppWindow } from "../appWindow";
import { HintBar } from "../console/HintBar";
import { useMode } from "../console/mode";
import { useWindowDrag } from "../useWindowDrag";
import { WindowControls } from "../WindowControls";

export function AccountPicker({
  list,
  onSignedIn,
}: {
  list: AccountList;
  onSignedIn?(account: Account): void;
}) {
  const { t } = useLingui();
  const consoleMode = useMode().mode === "console";
  const appWindow = useAppWindow();
  useWindowDrag(appWindow);
  const signIn = useSignIn();
  const [chosen, setChosen] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  // The list refreshes as accounts change (a lock starts, one is deleted).
  const asking = list.accounts.find((account) => account.id === chosen) ?? null;
  const last = lastAccountId();
  const start = list.accounts.find((account) => account.id === last) ?? list.accounts[0] ?? null;

  const signedIn = (account: Account) => {
    rememberAccount(account.id);
    onSignedIn?.(account);
  };
  const choose = (account: Account) => {
    setError(null);
    if (account.hasPin) {
      setChosen(account.id);
      return;
    }
    // Not `mutate`'s callbacks: the picker can be gone (signed in) by then.
    signIn.mutateAsync({ id: account.id, pin: null }).then(
      (answer) => {
        if (answer.type === "matched") signedIn(account);
      },
      (failure: Error) => setError(failure.message),
    );
  };

  // B leaves the PIN pad (which deletes digits first); on the accounts it
  // does nothing: there is nowhere behind them.
  useInputAction(
    "back",
    () => {
      setChosen(null);
      return true;
    },
    { label: asking ? t`All accounts` : undefined },
  );
  useInputAction("confirm", () => false, {
    label: t({
      message: "Select",
      context: "pad action",
      comment: "Hint bar: confirm the focused item",
    }),
  });

  return (
    <div
      data-cs="account-picker"
      data-step={asking ? "pin" : "accounts"}
      className="relative flex h-full flex-col overflow-hidden"
    >
      <div data-cs="backdrop" aria-hidden />
      <header
        data-part="titlebar"
        data-window-drag
        className="relative z-10 flex h-topbar shrink-0 items-center justify-end gap-2 pr-4 pl-6"
      >
        {appWindow?.frame === "custom" && !consoleMode && <WindowControls window={appWindow} />}
      </header>
      <main
        data-focus-section="main"
        className="relative z-10 flex min-h-0 flex-1 flex-col items-center overflow-y-auto px-6 pb-10"
      >
        {asking ? (
          <section
            key={asking.id}
            className="my-auto flex w-full animate-enter flex-col items-center gap-6"
          >
            <Button
              data-part="back"
              onPress={() => setChosen(null)}
              className="inline-flex items-center gap-2 self-center rounded-control px-3 py-1.5 text-sm text-fg-muted outline-none data-focus-visible:focus-ring data-hovered:bg-surface-hover data-hovered:text-fg"
            >
              <ArrowLeft className="size-4" /> <Trans>All accounts</Trans>
            </Button>
            <Avatar
              part="avatar"
              avatar={asking.avatar}
              name={asking.name}
              size={consoleMode ? "xl" : "lg"}
            />
            <PinPad
              title={t`Enter ${{ name: asking.name }}'s PIN`}
              lockedUntilMs={asking.lockedUntilMs}
              check={(pin) => signIn.mutateAsync({ id: asking.id, pin })}
              onMatched={() => signedIn(asking)}
              onCancel={() => setChosen(null)}
            />
          </section>
        ) : (
          <section className="my-auto flex w-full max-w-5xl flex-col items-center gap-10">
            <h1
              data-part="heading"
              className="animate-enter font-display text-3xl font-semibold tracking-wide"
            >
              <Trans>Who's playing?</Trans>
            </h1>
            <ul
              data-part="accounts"
              aria-label={t`Accounts`}
              className="flex flex-wrap justify-center gap-6 console:gap-10"
            >
              {list.accounts.map((account, index) => (
                <li
                  key={account.id}
                  className="animate-enter"
                  style={{ "--i": index } as CSSProperties}
                >
                  <AccountTile
                    account={account}
                    preferred={account.id === start?.id}
                    large={consoleMode}
                    isDisabled={signIn.isPending}
                    onPress={() => choose(account)}
                  />
                </li>
              ))}
            </ul>
            {error && (
              <p role="alert" className="text-sm text-danger">
                {error}
              </p>
            )}
          </section>
        )}
      </main>
      {consoleMode && <HintBar />}
    </div>
  );
}

function AccountTile({
  account,
  preferred,
  large,
  isDisabled,
  onPress,
}: {
  account: Account;
  preferred: boolean;
  large: boolean;
  isDisabled: boolean;
  onPress(): void;
}) {
  const { t } = useLingui();
  const ref = useRef<HTMLButtonElement>(null);
  useDefaultFocus(ref, preferred);
  return (
    <Button
      ref={ref}
      data-part="account"
      data-status={account.hasPin ? "locked" : undefined}
      data-focus-default={preferred || undefined}
      autoFocus={preferred}
      aria-label={account.hasPin ? t`${{ name: account.name }}, PIN locked` : account.name}
      isDisabled={isDisabled}
      onPress={onPress}
      className="group relative flex w-36 flex-col items-center gap-3 rounded-card p-3 outline-none transition-[background-color,scale] duration-(--cs-motion-duration-fast) data-focus-visible:focus-ring data-hovered:bg-surface-hover data-pressed:scale-[0.97] console:w-48"
    >
      <span className="relative">
        <Avatar
          part="avatar"
          avatar={account.avatar}
          name={account.name}
          size={large ? "xl" : "lg"}
        />
        {account.hasPin && (
          <span
            data-part="lock"
            aria-hidden
            className="absolute right-0 bottom-0 grid size-7 place-items-center rounded-full border border-border-strong bg-surface-overlay text-fg-muted"
          >
            <Lock className="size-3.5" />
          </span>
        )}
      </span>
      <span
        data-part="name"
        className="max-w-full truncate font-medium text-fg-muted group-data-focus-visible:text-fg group-data-hovered:text-fg"
      >
        {account.name}
      </span>
    </Button>
  );
}
