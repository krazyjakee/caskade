/** An account's PIN row (#208): set, change or remove it on the PIN pad.
 * An account changing its own PIN enters the current one first. */
import { Button, SettingRow } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { KeyRound } from "lucide-react";
import { useState } from "react";
import type { PinCheck } from "../../../api/accounts";
import { errorMessage } from "../../../api/reply";
import { ConfirmDialog } from "../../../components/ConfirmDialog";
import { PinDialog } from "./PinDialog";

/** The core's reason as a sentence: "Every admin needs a PIN…." */
export function sentence(error: unknown): string {
  const text = errorMessage(error);
  const capital = text.charAt(0).toUpperCase() + text.slice(1);
  return /[.!?]$/.test(capital) ? capital : `${capital}.`;
}

export function PinSetting({
  name,
  hasPin,
  lockedUntilMs,
  own,
  set,
  description,
}: {
  name: string;
  hasPin: boolean;
  lockedUntilMs?: number | null;
  /** Checks the current PIN, for an account changing its own. */
  own?: (pin: string) => Promise<PinCheck>;
  /** Stores the new PIN, or removes it with null; a refusal throws. */
  set(pin: string | null): unknown;
  description?: string;
}) {
  const { t } = useLingui();
  const [dialog, setDialog] = useState<"choose" | "remove" | "confirm-remove" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [removing, setRemoving] = useState(false);
  const save = async (pin: string | null) => {
    try {
      await set(pin);
      setError(null);
    } catch (refused) {
      throw new Error(sentence(refused));
    }
  };
  // Your own PIN goes once you enter it; another account's asks first, as
  // it opens that account to anyone at once (#49).
  const remove = () => {
    setError(null);
    setDialog(own ? "remove" : "confirm-remove");
  };
  const confirmRemove = () => {
    setRemoving(true);
    save(null)
      .then(() => setDialog(null))
      .catch((refused: Error) => setError(refused.message))
      .finally(() => setRemoving(false));
  };
  return (
    <>
      <SettingRow
        icon={KeyRound}
        label={t`PIN`}
        description={
          description ??
          (hasPin ? t`A PIN locks this account.` : t`No PIN: anyone can sign in to it.`)
        }
      >
        <Button size="sm" onPress={() => setDialog("choose")}>
          {hasPin ? <Trans>Change PIN</Trans> : <Trans>Set PIN</Trans>}
        </Button>
        {hasPin && (
          <Button size="sm" variant="ghost" onPress={remove}>
            <Trans>Remove PIN</Trans>
          </Button>
        )}
      </SettingRow>
      {error && dialog !== "confirm-remove" && (
        <p role="alert" className="pb-2 text-sm text-danger">
          {error}
        </p>
      )}
      {dialog === "confirm-remove" && (
        <ConfirmDialog
          title={t`Remove ${{ name }}'s PIN?`}
          confirmLabel={t`Remove PIN`}
          pendingLabel={t`Removing…`}
          isPending={removing}
          error={error}
          onConfirm={confirmRemove}
          close={() => {
            setError(null);
            setDialog(null);
          }}
        >
          <p>
            <Trans>
              Anyone can then sign in to {name} without a PIN. You can set a new one later.
            </Trans>
          </p>
        </ConfirmDialog>
      )}
      {(dialog === "choose" || dialog === "remove") && (
        <PinDialog
          name={name}
          current={hasPin ? own : undefined}
          lockedUntilMs={lockedUntilMs}
          remove={dialog === "remove"}
          save={save}
          close={() => setDialog(null)}
        />
      )}
    </>
  );
}
