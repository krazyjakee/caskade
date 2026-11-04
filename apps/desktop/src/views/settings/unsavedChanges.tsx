/** Explicit-save forms in Settings (#43). Most settings save as they change;
 * a form with a Save button holds its edits until it is pressed, and a
 * section's panel is replaced when another opens. Such a form registers here
 * while it has edits, and leaving its section (another section or
 * sub-section with a click, the keyboard or LB/RB, another page, or Back)
 * asks first: Save, Discard or Stay. */
import { Button } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { useBlocker } from "@tanstack/react-router";
import { CircleAlert } from "lucide-react";
import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useId,
  useRef,
  useState,
} from "react";
import { Dialog, Heading, Modal, ModalOverlay } from "react-aria-components";

export interface UnsavedForm {
  /** What holds the edits, named in the question (e.g. "My profile"). */
  label: string;
  /** Saves the edits: false, or a rejection, when they weren't saved. */
  save: () => boolean | Promise<boolean>;
  /** Drops the edits. */
  discard: () => void;
}

type Forms = Map<string, () => UnsavedForm>;
const UnsavedFormsContext = createContext<Forms | null>(null);

/** Registers a form while `dirty`, so leaving its section asks first. Outside
 * Settings (no guard above it) it does nothing. */
export function useUnsavedChanges(dirty: boolean, form: UnsavedForm) {
  const forms = useContext(UnsavedFormsContext);
  const id = useId();
  const latest = useRef(form);
  useEffect(() => {
    latest.current = form;
  });
  useEffect(() => {
    if (!forms || !dirty) return;
    forms.set(id, () => latest.current);
    return () => {
      forms.delete(id);
    };
  }, [forms, dirty, id]);
}

/** Says a form has edits that its Save button keeps, beside that button. */
export function UnsavedHint({ dirty, className = "" }: { dirty: boolean; className?: string }) {
  return (
    <span
      aria-live="polite"
      className={`inline-flex items-center gap-1.5 text-xs text-warning ${className}`}
    >
      {dirty && (
        <>
          <CircleAlert aria-hidden className="size-3.5 shrink-0" />
          <Trans>Unsaved changes</Trans>
        </>
      )}
    </span>
  );
}

interface Place {
  pathname: string;
  search: unknown;
}

/** The section a location shows: its page, tab and sub-section. */
function sectionOf({ pathname, search }: Place) {
  const values = (search ?? {}) as Record<string, unknown>;
  return `${pathname}\n${String(values.tab ?? "")}\n${String(values.section ?? "")}`;
}

/** Around Settings' sections: asks before a navigation replaces a panel
 * whose forms have edits. */
export function UnsavedChangesGuard({ children }: { children: ReactNode }) {
  const [forms] = useState<Forms>(() => new Map());
  const shouldBlockFn = useCallback(
    ({ current, next }: { current: Place; next: Place }) =>
      forms.size > 0 && sectionOf(current) !== sectionOf(next),
    [forms],
  );
  const blocker = useBlocker({ shouldBlockFn, withResolver: true, enableBeforeUnload: false });
  return (
    <UnsavedFormsContext.Provider value={forms}>
      {children}
      {blocker.status === "blocked" && (
        <UnsavedChangesDialog
          forms={[...forms.values()].map((form) => form())}
          leave={blocker.proceed}
          stay={blocker.reset}
        />
      )}
    </UnsavedFormsContext.Provider>
  );
}

function UnsavedChangesDialog({
  forms,
  leave,
  stay,
}: {
  forms: UnsavedForm[];
  leave: () => void;
  stay: () => void;
}) {
  const { t } = useLingui();
  const [saving, setSaving] = useState(false);
  const [failed, setFailed] = useState(false);
  const save = async () => {
    setSaving(true);
    setFailed(false);
    let saved = true;
    for (const form of forms) {
      try {
        if (!(await form.save())) saved = false;
      } catch {
        saved = false;
      }
    }
    setSaving(false);
    if (saved) leave();
    else setFailed(true);
  };
  const discard = () => {
    for (const form of forms) form.discard();
    leave();
  };
  return (
    <ModalOverlay
      isOpen
      isDismissable={!saving}
      onOpenChange={(open) => {
        if (!open && !saving) stay();
      }}
      className="fixed inset-0 z-50 grid place-items-center bg-surface-base/80 p-4 backdrop-blur-sm"
    >
      <Modal className="w-full max-w-md rounded-panel border border-border-strong bg-surface-raised p-6 shadow-lg">
        <Dialog role="alertdialog" className="space-y-4 outline-none">
          <Heading slot="title" className="font-display text-xl font-semibold">
            <Trans>Save your changes?</Trans>
          </Heading>
          <p className="text-sm text-fg-muted">
            <Trans>
              These changes aren't saved yet. Leaving this section without saving loses them.
            </Trans>
          </p>
          <ul className="list-inside list-disc text-sm">
            {forms.map((form) => (
              <li key={form.label}>{form.label}</li>
            ))}
          </ul>
          {failed && (
            <p role="alert" className="text-sm text-danger">
              <Trans>
                Some changes couldn't be saved. Stay to fix them, or discard them and leave.
              </Trans>
            </p>
          )}
          <div className="flex flex-wrap justify-end gap-2">
            <Button data-spatial="focusable" variant="danger" isDisabled={saving} onPress={discard}>
              <Trans comment="Leave a settings section without saving its changes">Discard</Trans>
            </Button>
            <Button data-spatial="focusable" data-focus-default isDisabled={saving} onPress={stay}>
              <Trans comment="Keep editing the unsaved settings">Stay</Trans>
            </Button>
            <Button
              data-spatial="focusable"
              variant="primary"
              isDisabled={saving}
              onPress={() => void save()}
            >
              {saving ? t`Saving…` : t`Save`}
            </Button>
          </div>
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}
