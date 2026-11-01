/** Adds an account or changes one (#208), for admins: its name, avatar,
 * role, PIN, what it may change and its age limit. The core keeps the
 * rules (an admin remains; every admin has a PIN while standard accounts
 * exist) and the editor shows its reason when a save breaks one. */
import {
  AvatarPicker,
  Button,
  SegmentedControl,
  Select,
  SettingRow,
  Switch,
  TextField,
} from "@caskade/ui";
import { msg } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { Download, ShieldCheck, ShieldQuestionMark, User, UserCog } from "lucide-react";
import { useId, useState } from "react";
import { Dialog, Form, Heading, Modal, ModalOverlay } from "react-aria-components";
import {
  type Account,
  type AccountAccess,
  accessOf,
  useAccountsApi,
  useCreateAccount,
  useSetPin,
  useUpdateAccount,
} from "../../../api/accounts";
import { usePreferredBoard } from "../../../components/AgeRatingLabel";
import { ageLimitOptions, NO_LIMIT } from "./ageLimits";
import { PinSetting, sentence } from "./PinSetting";

/** The Settings sections a standard account may be allowed to change
 * (`caskade_core::accounts::SETTINGS_SECTIONS`), as the tabs name them. */
const permissionSections = [
  { id: "general", label: msg`General settings` },
  { id: "library-discovery", label: msg`Library Discovery settings` },
  { id: "launchers", label: msg`Launchers settings` },
  { id: "emulators", label: msg`Emulation settings` },
  { id: "appearance", label: msg`Appearance settings` },
  { id: "storage", label: msg`Storage settings` },
  { id: "installer", label: msg`Installer settings` },
  { id: "plugins", label: msg`Plugins settings` },
] as const;

const roles = [
  { id: "standard", label: msg({ message: "Standard", context: "account role" }), icon: User },
  { id: "admin", label: msg({ message: "Admin", context: "account role" }), icon: UserCog },
] as const;

const newAccess: AccountAccess = {
  role: "standard",
  permissions: { settings: [], install: false },
  maxAge: null,
  unratedNeedsPin: false,
};

export function AccountEditor({
  account,
  activeId,
  close,
}: {
  /** The account to change; a new one when absent. */
  account?: Account;
  activeId: string | null;
  close: () => void;
}) {
  const { t } = useLingui();
  const api = useAccountsApi();
  const board = usePreferredBoard();
  const create = useCreateAccount();
  const update = useUpdateAccount();
  const setPin = useSetPin();
  const avatarLabel = useId();
  const [name, setName] = useState(account?.name ?? "");
  const [avatar, setAvatar] = useState(account?.avatar ?? "avatar:default");
  const [access, setAccess] = useState<AccountAccess>(() =>
    account ? accessOf(account) : newAccess,
  );
  // A new account's PIN waits here until it is created.
  const [pin, setDraftPin] = useState<string | null>(null);
  const save = account ? update : create;
  const busy = save.isPending;
  const named = name.trim().length > 0;
  const standard = access.role === "standard";
  const own = account !== undefined && account.id === activeId;
  const allowed = (section: string) => access.permissions.settings.includes(section);
  const allow = (section: string, on: boolean) =>
    setAccess((current) => ({
      ...current,
      permissions: {
        ...current.permissions,
        settings: on
          ? [...current.permissions.settings, section]
          : current.permissions.settings.filter((id) => id !== section),
      },
    }));
  const submit = () => {
    // An admin may change everything; age limits are for standard accounts.
    const sent: AccountAccess = standard
      ? access
      : { ...access, maxAge: null, unratedNeedsPin: false };
    const done = { onSuccess: () => close() };
    if (account)
      update.mutate({ id: account.id, update: { name: name.trim(), avatar, access: sent } }, done);
    else create.mutate({ name: name.trim(), avatar, access: sent, pin }, done);
  };
  return (
    <ModalOverlay
      isOpen
      isDismissable={!busy}
      onOpenChange={(open) => {
        if (!open && !busy) close();
      }}
      className="fixed inset-0 z-50 grid place-items-center bg-surface-base/80 p-4 backdrop-blur-sm"
    >
      <Modal className="max-h-[90vh] w-full max-w-2xl overflow-auto rounded-panel border border-border-strong bg-surface-raised p-6 shadow-lg">
        <Dialog data-cs="account-editor" className="outline-none">
          <Heading slot="title" className="font-display text-xl font-semibold">
            {account ? t`Edit ${{ name: account.name }}` : t`Add account`}
          </Heading>
          <Form
            className="mt-4 flex flex-col gap-4"
            onSubmit={(event) => {
              event.preventDefault();
              submit();
            }}
          >
            <TextField
              label={t({ message: "Name", context: "account" })}
              data-focus-default
              autoFocus
              value={name}
              onChange={setName}
              isRequired
              maxLength={64}
              isDisabled={busy}
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
                isDisabled={busy}
              />
            </div>
            <div>
              <SettingRow
                icon={UserCog}
                label={t({ message: "Role", context: "account" })}
                description={t`An admin changes everything, other accounts included.`}
              >
                <SegmentedControl
                  options={roles.map((role) => ({ ...role, label: t(role.label) }))}
                  value={access.role}
                  onChange={(role) => setAccess((current) => ({ ...current, role }))}
                  isDisabled={busy}
                />
              </SettingRow>
              {account ? (
                <PinSetting
                  name={account.name}
                  hasPin={account.hasPin}
                  lockedUntilMs={account.lockedUntilMs}
                  own={own && api ? (entered) => api.signIn(account.id, entered) : undefined}
                  set={(next) => setPin.mutateAsync({ id: account.id, pin: next })}
                  description={
                    account.hasPin
                      ? t`A PIN locks this account. Changes at once.`
                      : t`No PIN: anyone can sign in to it. Changes at once.`
                  }
                />
              ) : (
                <PinSetting
                  name={
                    name.trim() ||
                    t({
                      message: "the account",
                      comment:
                        "Fills in for a new account's name: 'Choose a new PIN for the account'",
                    })
                  }
                  hasPin={pin !== null}
                  set={(next) => setDraftPin(next)}
                />
              )}
            </div>
            {standard && (
              <fieldset data-part="access" className="flex flex-col">
                <legend className="mb-1 text-sm font-medium">
                  <Trans comment="Over a standard account's permissions">What it can do</Trans>
                </legend>
                <SettingRow
                  icon={Download}
                  label={t`Install games`}
                  description={t`Install, uninstall and move games. Updates run for everyone.`}
                >
                  <Switch
                    isSelected={access.permissions.install}
                    onChange={(install) =>
                      setAccess((current) => ({
                        ...current,
                        permissions: { ...current.permissions, install },
                      }))
                    }
                    isDisabled={busy}
                  />
                </SettingRow>
                {permissionSections.map((section) => (
                  <SettingRow
                    key={section.id}
                    label={t(section.label)}
                    description={t`Change this Settings section.`}
                  >
                    <Switch
                      isSelected={allowed(section.id)}
                      onChange={(on) => allow(section.id, on)}
                      isDisabled={busy}
                    />
                  </SettingRow>
                ))}
                <SettingRow
                  icon={ShieldCheck}
                  label={t`Maximum age rating`}
                  description={t`Games rated above it need an admin's PIN.`}
                >
                  <Select
                    options={ageLimitOptions(board, access.maxAge)}
                    value={access.maxAge === null ? NO_LIMIT : String(access.maxAge)}
                    onChange={(id) =>
                      setAccess((current) => ({
                        ...current,
                        maxAge: id === NO_LIMIT ? null : Number(id),
                      }))
                    }
                    isDisabled={busy}
                  />
                </SettingRow>
                <SettingRow
                  icon={ShieldQuestionMark}
                  label={t`Unrated games need a PIN`}
                  description={t`Under an age limit, games without a rating need an admin's PIN too.`}
                >
                  <Switch
                    isSelected={access.unratedNeedsPin}
                    onChange={(unratedNeedsPin) =>
                      setAccess((current) => ({ ...current, unratedNeedsPin }))
                    }
                    isDisabled={busy || access.maxAge === null}
                  />
                </SettingRow>
              </fieldset>
            )}
            {save.error && (
              <p role="alert" className="text-sm text-danger">
                {sentence(save.error)}
              </p>
            )}
            <div className="flex justify-end gap-2">
              <Button onPress={close} isDisabled={busy}>
                <Trans>Cancel</Trans>
              </Button>
              <Button type="submit" variant="primary" isDisabled={!named || busy}>
                {busy ? (
                  <Trans>Saving…</Trans>
                ) : account ? (
                  <Trans>Save</Trans>
                ) : (
                  <Trans comment="Adds the new account">Add</Trans>
                )}
              </Button>
            </div>
          </Form>
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}
