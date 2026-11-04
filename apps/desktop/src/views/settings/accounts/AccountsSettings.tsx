/** Settings → Accounts (#208), for admins: every account with Edit and
 * Delete, Add account, and the rating board every account's ratings and
 * age limits use. The tab is hidden from standard accounts, and the core
 * refuses their changes whatever this shows. */
import { Avatar, Button, Select, SettingRow, SettingsSection } from "@caskade/ui";
import { t as translate } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { Pencil, Plus, ShieldCheck, Trash2, Users } from "lucide-react";
import { useState } from "react";
import { Dialog, Heading, Modal, ModalOverlay } from "react-aria-components";
import { type Account, useAccounts, useDeleteAccount } from "../../../api/accounts";
import {
  ACCOUNTS_SETTINGS,
  BOARDS,
  boardName,
  DEFAULT_BOARD,
} from "../../../components/AgeRatingLabel";
import { FailureNotice } from "../../../components/FailureNotice";
import { SectionSync } from "../devices/SectionSync";
import { useSettingsSection } from "../useSettings";
import { AccountEditor } from "./AccountEditor";
import { ageLimitLabel } from "./ageLimits";
import { sentence } from "./PinSetting";

type Editing = { type: "add" } | { type: "edit" | "delete"; id: string };

const boardOptions = BOARDS.map((id) => ({ id, label: boardName(id) }));

/** "Standard · PIN · installs games · PEGI 12", in the current language. */
function details(account: Account, board: string): string {
  const parts = [
    account.role === "admin"
      ? translate({ message: "Admin", context: "account role" })
      : translate({ message: "Standard", context: "account role" }),
    account.hasPin
      ? translate({ message: "PIN", context: "account details" })
      : translate({ message: "no PIN", context: "account details" }),
    account.role !== "admin" && account.permissions.install
      ? translate({ message: "installs games", context: "account details" })
      : null,
    account.role === "admin" || account.maxAge === null
      ? null
      : ageLimitLabel(board, account.maxAge),
  ];
  return parts.filter(Boolean).join(" · ");
}

function DeleteAccountDialog({ account, close }: { account: Account; close: () => void }) {
  const { t } = useLingui();
  const remove = useDeleteAccount();
  return (
    <ModalOverlay
      isOpen
      isDismissable={!remove.isPending}
      onOpenChange={(open) => {
        if (!open && !remove.isPending) close();
      }}
      className="fixed inset-0 z-50 grid place-items-center bg-surface-base/80 p-4 backdrop-blur-sm"
    >
      <Modal className="w-full max-w-md rounded-panel border border-border-strong bg-surface-raised p-6 shadow-lg">
        <Dialog role="alertdialog" className="space-y-4 outline-none">
          <Heading slot="title" className="font-display text-xl font-semibold">
            {t`Delete ${{ name: account.name }}?`}
          </Heading>
          <p className="text-sm text-fg-muted">
            {t`${{ name: account.name }}'s collections go with it. Games, installs and play history stay.`}
          </p>
          {remove.error && (
            <p role="alert" className="text-sm text-danger">
              {sentence(remove.error)}
            </p>
          )}
          <div className="flex justify-end gap-2">
            <Button
              data-spatial="focusable"
              data-focus-default
              isDisabled={remove.isPending}
              onPress={close}
            >
              <Trans>Cancel</Trans>
            </Button>
            <Button
              data-spatial="focusable"
              variant="danger"
              isDisabled={remove.isPending}
              onPress={() => remove.mutate(account.id, { onSuccess: close })}
            >
              {remove.isPending ? <Trans>Deleting…</Trans> : <Trans>Delete</Trans>}
            </Button>
          </div>
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}

export function AccountsSettings() {
  const { t } = useLingui();
  const accounts = useAccounts();
  const ratings = useSettingsSection<{ ratingBoard: string }>(ACCOUNTS_SETTINGS);
  const board = ratings.settings?.ratingBoard ?? DEFAULT_BOARD;
  const [editing, setEditing] = useState<Editing | null>(null);
  const list = accounts.data?.accounts ?? [];
  const activeId = accounts.data?.activeId ?? null;
  const target =
    editing && editing.type !== "add" ? list.find((account) => account.id === editing.id) : null;
  const close = () => setEditing(null);
  // A failed list says so with Try again; a loading one says it's loading (#57).
  if (accounts.error)
    return (
      <FailureNotice
        error={accounts.error}
        title={t`Accounts couldn't be loaded`}
        onRetry={() => void accounts.refetch()}
      />
    );
  if (accounts.isPending)
    return (
      <p role="status" className="text-sm text-fg-muted">
        <Trans>Loading accounts…</Trans>
      </p>
    );
  const admins = list.filter((account) => account.role === "admin").length;
  /** Deleting yourself or the last admin is refused: not offered. */
  const deletable = (account: Account) =>
    account.id !== activeId && !(account.role === "admin" && admins === 1);
  return (
    <div data-settings="accounts" className="flex flex-col gap-4">
      <SettingsSection
        icon={Users}
        title={t({ message: "Accounts", context: "settings tab" })}
        description={t`Who uses Caskade here, and what each can change.`}
        aside={
          <Button size="sm" icon={Plus} onPress={() => setEditing({ type: "add" })}>
            <Trans>Add account…</Trans>
          </Button>
        }
      >
        <ul
          data-cs="account-list"
          aria-label={t({ message: "Accounts", context: "settings tab" })}
          className="flex flex-col"
        >
          {list.map((account) => (
            <li
              key={account.id}
              data-part="account"
              className="flex items-center gap-3 border-t border-border py-3 first:border-t-0"
            >
              <Avatar part="avatar" avatar={account.avatar} name={account.name} size="md" />
              <div className="min-w-0 flex-1">
                <div data-part="name" className="truncate text-sm font-medium">
                  {account.name}
                  {account.id === activeId && (
                    <span className="text-fg-subtle">
                      {" "}
                      <Trans comment="After the signed-in account's name">(you)</Trans>
                    </span>
                  )}
                </div>
                <div data-part="details" className="text-xs text-fg-muted">
                  {details(account, board)}
                </div>
              </div>
              <Button
                data-part="edit"
                size="sm"
                icon={Pencil}
                aria-label={t`Edit ${{ name: account.name }}`}
                onPress={() => setEditing({ type: "edit", id: account.id })}
              >
                <Trans>Edit…</Trans>
              </Button>
              {deletable(account) && (
                <Button
                  data-part="delete"
                  size="sm"
                  variant="ghost"
                  icon={Trash2}
                  aria-label={t`Delete ${{ name: account.name }}`}
                  onPress={() => setEditing({ type: "delete", id: account.id })}
                >
                  <Trans>Delete…</Trans>
                </Button>
              )}
            </li>
          ))}
        </ul>
      </SettingsSection>
      <SettingsSection
        icon={ShieldCheck}
        title={t`Age ratings`}
        description={t`For every account. Age limits are set on each account.`}
      >
        {ratings.settings && (
          <SettingRow
            label={t`Rating board`}
            description={t`The ratings games show, and the steps age limits are picked from.`}
          >
            <Select
              options={boardOptions}
              value={board as (typeof BOARDS)[number]}
              onChange={(ratingBoard) =>
                ratings.setSettings((current) => ({ ...current, ratingBoard }))
              }
              isDisabled={ratings.saving}
            />
          </SettingRow>
        )}
        {ratings.error && (
          <p role="alert" className="text-sm text-danger">
            {sentence(ratings.error)}
          </p>
        )}
        <SectionSync namespace="core.accounts" />
      </SettingsSection>
      {editing?.type === "add" && <AccountEditor activeId={activeId} close={close} />}
      {editing?.type === "edit" && target && (
        <AccountEditor account={target} activeId={activeId} close={close} />
      )}
      {editing?.type === "delete" && target && (
        <DeleteAccountDialog account={target} close={close} />
      )}
    </div>
  );
}
