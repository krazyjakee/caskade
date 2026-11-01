/** Settings → My profile (#208), for every account: its own name, avatar
 * and PIN. Changing or removing a PIN asks for the current one. */
import { AvatarPicker, Button, SettingsSection, TextField } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { UserRound } from "lucide-react";
import { useEffect, useId, useState } from "react";
import {
  type Account,
  useAccountsApi,
  useActiveAccount,
  useSetPin,
  useUpdateAccount,
} from "../../../api/accounts";
import { PinSetting, sentence } from "./PinSetting";

function Profile({ account }: { account: Account }) {
  const { t } = useLingui();
  const api = useAccountsApi();
  const update = useUpdateAccount();
  const setPin = useSetPin();
  const avatarLabel = useId();
  const [name, setName] = useState(account.name);
  const [avatar, setAvatar] = useState(account.avatar);
  // Saved elsewhere (another window, an admin): show what was saved.
  useEffect(() => setName(account.name), [account.name]);
  useEffect(() => setAvatar(account.avatar), [account.avatar]);
  const named = name.trim().length > 0;
  const changed = name.trim() !== account.name || avatar !== account.avatar;
  return (
    <SettingsSection
      icon={UserRound}
      title={t({ message: "My profile", context: "settings tab" })}
      description={
        account.role === "admin"
          ? t`Signed in as ${{ name: account.name }}, an admin.`
          : t`Signed in as ${{ name: account.name }}, a standard account.`
      }
    >
      <div data-cs="account-profile" className="flex flex-col gap-4 pt-2">
        <form
          className="flex flex-col gap-4"
          onSubmit={(event) => {
            event.preventDefault();
            if (named && changed)
              update.mutate({
                id: account.id,
                update: { name: name.trim(), avatar, access: null },
              });
          }}
        >
          <TextField
            label={t({ message: "Name", context: "account" })}
            value={name}
            onChange={setName}
            isRequired
            maxLength={64}
            isDisabled={update.isPending}
          />
          <div data-part="avatars" className="flex flex-col gap-2">
            <span id={avatarLabel} className="text-sm font-medium">
              <Trans>Avatar</Trans>
            </span>
            <AvatarPicker
              aria-labelledby={avatarLabel}
              value={avatar}
              onChange={setAvatar}
              size="md"
              isDisabled={update.isPending}
            />
          </div>
          {update.error && (
            <p role="alert" className="text-sm text-danger">
              {sentence(update.error)}
            </p>
          )}
          <div className="flex justify-end">
            <Button
              type="submit"
              variant="primary"
              isDisabled={!named || !changed || update.isPending}
            >
              {update.isPending ? <Trans>Saving…</Trans> : <Trans>Save</Trans>}
            </Button>
          </div>
        </form>
        <div data-part="pin">
          <PinSetting
            name={account.name}
            hasPin={account.hasPin}
            lockedUntilMs={account.lockedUntilMs}
            own={api ? (pin) => api.signIn(account.id, pin) : undefined}
            set={(pin) => setPin.mutateAsync({ id: account.id, pin })}
          />
        </div>
      </div>
    </SettingsSection>
  );
}

export function ProfileSettings() {
  const account = useActiveAccount();
  if (account === undefined)
    return (
      <p className="text-fg-muted">
        <Trans>Loading your profile…</Trans>
      </p>
    );
  if (account === null)
    return (
      <p className="text-fg-muted">
        <Trans>Sign in to change your profile.</Trans>
      </p>
    );
  return (
    <div data-settings="profile" className="max-w-3xl">
      <Profile key={account.id} account={account} />
    </div>
  );
}
