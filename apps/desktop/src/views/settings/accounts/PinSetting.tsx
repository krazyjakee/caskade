/** An account's PIN row (#208): set, change or remove it on the PIN pad.
 * An account changing its own PIN enters the current one first. */
import { Button, SettingRow } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { KeyRound } from "lucide-react";
import { useState } from "react";
import type { PinCheck } from "../../../api/accounts";
import { errorMessage } from "../../../api/reply";
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
  const [dialog, setDialog] = useState<"choose" | "remove" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const save = async (pin: string | null) => {
    try {
      await set(pin);
      setError(null);
    } catch (refused) {
      throw new Error(sentence(refused));
    }
  };
  const remove = () => {
    if (own) {
      setDialog("remove");
      return;
    }
    setError(null);
    save(null).catch((refused: Error) => setError(refused.message));
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
      {error && (
        <p role="alert" className="pb-2 text-sm text-danger">
          {error}
        </p>
      )}
      {dialog && (
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
