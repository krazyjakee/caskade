/** The signed-in account's avatar in the top bar (#207), in both shells: it
 * opens a menu that says who is signed in and offers My profile (Settings →
 * My profile) and Switch account, which signs out to the account picker.
 * Not while a game Caskade launched is running: the session belongs to the
 * account that started it. */
import { Avatar, Menu, MenuItem, MenuPopover } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { useNavigate } from "@tanstack/react-router";
import { UserRound, Users } from "lucide-react";
import { Button, MenuTrigger } from "react-aria-components";
import { useActiveAccount, useSignOut } from "../../api/accounts";
import { useRunningCount } from "../../api/hooks";

export function AccountMenu({ size }: { size: "sm" | "md" }) {
  const { t } = useLingui();
  const account = useActiveAccount();
  const running = useRunningCount();
  const signOut = useSignOut();
  const navigate = useNavigate();
  if (!account) return null;
  const playing = (running.data ?? 0) > 0;
  return (
    <MenuTrigger>
      <Button
        data-part="avatar"
        aria-label={t`${{ name: account.name }}, account`}
        className="flex rounded-full outline-none transition-[filter] duration-(--cs-motion-duration-fast) data-focus-visible:focus-ring data-hovered:brightness-115"
      >
        <Avatar avatar={account.avatar} name={account.name} size={size} />
      </Button>
      <MenuPopover placement="bottom end">
        <div
          data-cs="account-menu"
          className="mb-1 flex items-center gap-3 border-b border-border px-2.5 pt-1.5 pb-2.5"
        >
          <Avatar part="avatar" avatar={account.avatar} name={account.name} size="md" />
          <div className="flex min-w-0 flex-col">
            <span data-part="name" className="truncate font-medium text-fg">
              {account.name}
            </span>
            <span data-part="role" className="text-xs text-fg-subtle">
              {account.role === "admin"
                ? t({ message: "Admin", context: "account role" })
                : t({ message: "Standard", context: "account role" })}
            </span>
          </div>
        </div>
        <Menu
          aria-label={t`${{ name: account.name }}'s account`}
          onAction={(id) => {
            if (id === "profile") void navigate({ to: "/settings", search: { tab: "profile" } });
            if (id === "switch") signOut.mutate(undefined);
          }}
        >
          <MenuItem id="profile" icon={UserRound}>
            {t({ message: "My profile", context: "settings tab" })}
          </MenuItem>
          <MenuItem
            id="switch"
            icon={Users}
            isDisabled={playing}
            description={playing ? t`Not while a game is running.` : undefined}
          >
            <Trans>Switch account</Trans>
          </MenuItem>
        </Menu>
      </MenuPopover>
    </MenuTrigger>
  );
}
