/** Choosing a PIN (#208) on the shared PIN pad, so a controller sets one
 * too: the current PIN first when an account changes its own, then the new
 * one twice. Removing one asks only for the current PIN. A PIN lives only
 * in this dialog until `save` hands it on. */
import { useLingui } from "@lingui/react/macro";
import { useState } from "react";
import { Dialog, Modal, ModalOverlay } from "react-aria-components";
import type { PinCheck } from "../../../api/accounts";
import { PinPad } from "../../../components/accounts/PinPad";

type Step = "current" | "new" | "again";

export function PinDialog({
  name,
  current,
  lockedUntilMs,
  remove = false,
  save,
  close,
}: {
  /** Whose PIN: "Ada". */
  name: string;
  /** Checks the account's current PIN; when given, it is asked first. */
  current?: (pin: string) => Promise<PinCheck>;
  lockedUntilMs?: number | null;
  /** Removes the PIN instead of choosing a new one. */
  remove?: boolean;
  /** Sets the new PIN, or null to remove it; its error shows on the pad. */
  save(pin: string | null): unknown;
  close(): void;
}) {
  const { t } = useLingui();
  const [step, setStep] = useState<Step>(current ? "current" : "new");
  const [first, setFirst] = useState("");
  const matched: PinCheck = { type: "matched" };
  const title =
    step === "current"
      ? t`Enter ${{ name }}'s current PIN`
      : step === "new"
        ? t`Choose a new PIN for ${{ name }}`
        : t`Enter the new PIN again`;
  const check = async (pin: string): Promise<PinCheck> => {
    if (step === "current") {
      const answer = current ? await current(pin) : matched;
      if (answer.type === "matched" && remove) await save(null);
      return answer;
    }
    if (step === "new") {
      setFirst(pin);
      return matched;
    }
    if (pin !== first) throw new Error(t`The PINs don't match. Try again.`);
    await save(pin);
    return matched;
  };
  const next = () => {
    if (step === "current" && !remove) setStep("new");
    else if (step === "new") setStep("again");
    else close();
  };
  return (
    <ModalOverlay
      isOpen
      isDismissable
      onOpenChange={(open) => {
        if (!open) close();
      }}
      className="fixed inset-0 z-50 grid place-items-center bg-surface-base/80 p-4 backdrop-blur-sm"
    >
      <Modal className="w-full max-w-sm rounded-panel border border-border-strong bg-surface-raised p-6 shadow-lg">
        <Dialog aria-label={title} className="outline-none">
          {/* React carries the pad's submit through the portal to a form
              that opened this dialog (the account editor's); it stops here. */}
          <div className="grid place-items-center" onSubmit={(event) => event.stopPropagation()}>
            <PinPad
              key={step}
              title={title}
              check={check}
              onMatched={next}
              onCancel={close}
              lockedUntilMs={step === "current" ? lockedUntilMs : null}
            />
          </div>
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}
