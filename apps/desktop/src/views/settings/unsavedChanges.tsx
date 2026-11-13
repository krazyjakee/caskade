/** Explicit-save forms in Settings (#43). Most settings save as they change;
 * a form with a Save button holds its edits until it is pressed, and a
 * section's panel is replaced when another opens. Such a form registers here
 * while it has edits, and leaving its section (another section or
 * sub-section with a click, the keyboard or LB/RB, another page, or Back)
 * asks first: Save, Discard or Stay. So do switching accounts and modes
 * (#58), which replace Settings without a navigation, and a close of the
 * window that would quit (`useCloseQuestion`). */

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
  useMemo,
  useRef,
  useState,
} from "react";
import { Dialog, Heading } from "react-aria-components";
import { useRunningCount } from "../../api/hooks";
import { ModalFrame } from "../../components/ModalFrame";
import { useAppWindow } from "../../shell/appWindow";

interface UnsavedForm {
  /** What holds the edits, named in the question (e.g. "My profile"). */
  label: string;
  /** Saves the edits: false, or a rejection, when they weren't saved. */
  save: () => boolean | Promise<boolean>;
  /** Drops the edits. */
  discard: () => void;
}

type Forms = Map<string, () => UnsavedForm>;
const UnsavedFormsContext = createContext<{ forms: Forms; changed: () => void } | null>(null);

/** Registers a form while `dirty`, so leaving its section asks first. Outside
 * Settings (no guard above it) it does nothing. */
export function useUnsavedChanges(dirty: boolean, form: UnsavedForm) {
  const registry = useContext(UnsavedFormsContext);
  const id = useId();
  const latest = useRef(form);
  useEffect(() => {
    latest.current = form;
  });
  useEffect(() => {
    if (!registry || !dirty) return;
    registry.forms.set(id, () => latest.current);
    registry.changed();
    return () => {
      registry.forms.delete(id);
      registry.changed();
    };
  }, [registry, dirty, id]);
}

/** What the edits would be lost to, beyond a navigation. */
type Leaving = "account" | "mode" | "window";

/** Asks about the guard's forms, then calls `proceed` unless the answer was
 * Stay. */
let askOnScreen: ((leaving: Leaving, proceed: () => void) => void) | null = null;

/** For what replaces Settings without a navigation, from outside the guard:
 * runs `proceed` straight away when no form has edits or Settings isn't
 * open, otherwise once the question is answered with Save or Discard. */
export function afterUnsavedChanges(leaving: "account" | "mode", proceed: () => void) {
  if (askOnScreen) askOnScreen(leaving, proceed);
  else proceed();
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

/** A question open about the edits: `leave` follows Save or Discard. */
interface Question {
  leaving: Leaving;
  leave: () => void;
  stay?: () => void;
}

/** How long the adapter's answer to a close takes to reach the window: Tauri
 * tells the webview first, and GTK hides or shows the window a turn of its
 * loop later. */
const CLOSE_LANDS_MS = 100;

/** Asks before a close of the window quits, while `dirty`. Only then: while
 * the webview listens, Tauri refuses every close, and the hold has the adapter
 * raise the window and stay in console mode until the answer. A hide to the
 * tray keeps the webview and its draft, so it asks nothing. The adapter waits
 * only for a question someone can see: under a game, or where the window
 * can't come to the front, nothing tells it the close was heard and it quits. */
function useCloseQuestion(
  dirty: boolean,
  question: Question | null,
  setQuestion: (question: Question | null) => void,
) {
  const appWindow = useAppWindow();
  const running = useRunningCount();
  const listening = useRef<Promise<() => void> | null>(null);
  const released = useRef(Promise.resolve());

  const release = useCallback(() => {
    const listener = listening.current;
    listening.current = null;
    if (listener)
      released.current = listener
        .then((stop) => stop())
        .then(() => appWindow?.holdClose(false))
        .catch((error: unknown) => console.warn("closes still held", error));
    return released.current;
  }, [appWindow]);

  const heard = useCallback(() => {
    appWindow?.holdClose(true).catch((error: unknown) => console.warn("close not answered", error));
  }, [appWindow]);

  const letThrough = () => {
    release()
      .then(() => appWindow?.close())
      .catch((error: unknown) => console.warn("window not closed", error));
  };

  const stay = () => {
    if (!appWindow || !listening.current) return;
    appWindow
      .holdClose(false)
      .then(() => appWindow.holdClose(true))
      .catch((error: unknown) => console.warn("quit not forgotten", error));
  };

  const closing = useRef(async () => {});
  useEffect(() => {
    closing.current = async () => {
      await new Promise((landed) => setTimeout(landed, CLOSE_LANDS_MS));
      const presence = await appWindow?.presence().catch(() => null);
      // Hidden to the tray; or the edits went meanwhile, or the guard did.
      if (!presence?.visible || !listening.current) return;
      setQuestion({ leaving: "window", leave: letThrough, stay });
      const { minimized, focused } = presence;
      // A window can lose focus to the tray's menu and still be in plain
      // view, but with a game running it may be under the game.
      if (!minimized && (focused || !(await running.refetch()).data)) heard();
    };
  });

  useEffect(() => {
    if (!appWindow || !dirty) return;
    listening.current = released.current.then(async () => {
      await appWindow.holdClose(true);
      return appWindow.onCloseRequested(() => void closing.current());
    });
    return () => void release();
  }, [appWindow, dirty, release]);

  // The raised window came to the front after all: its question is seen.
  const asking = question?.leaving === "window";
  useEffect(() => {
    if (!asking || !appWindow?.onFocusChanged) return;
    const unsubscribe = appWindow.onFocusChanged((focused) => {
      if (focused) heard();
    });
    return () => void unsubscribe.then((stop) => stop());
  }, [appWindow, asking, heard]);
}

/** Around Settings' sections: asks before a navigation replaces a panel
 * whose forms have edits, and before an account switch or a close of the
 * window drops them. */
export function UnsavedChangesGuard({ children }: { children: ReactNode }) {
  const [forms] = useState<Forms>(() => new Map());
  const [dirty, setDirty] = useState(false);
  const [question, setQuestion] = useState<Question | null>(null);
  const registry = useMemo(() => ({ forms, changed: () => setDirty(forms.size > 0) }), [forms]);
  const shouldBlockFn = useCallback(
    ({ current, next }: { current: Place; next: Place }) =>
      forms.size > 0 && sectionOf(current) !== sectionOf(next),
    [forms],
  );
  const blocker = useBlocker({ shouldBlockFn, withResolver: true, enableBeforeUnload: false });
  useCloseQuestion(dirty, question, setQuestion);
  useEffect(() => {
    const ask = (leaving: Leaving, proceed: () => void) => {
      if (forms.size === 0) proceed();
      else setQuestion({ leaving, leave: proceed });
    };
    askOnScreen = ask;
    return () => {
      if (askOnScreen === ask) askOnScreen = null;
    };
  }, [forms]);
  const held = () => [...forms.values()].map((form) => form());
  return (
    <UnsavedFormsContext.Provider value={registry}>
      {children}
      {question ? (
        <UnsavedChangesDialog
          key={question.leaving}
          leaving={question.leaving}
          forms={held()}
          leave={() => {
            setQuestion(null);
            question.leave();
          }}
          stay={() => {
            setQuestion(null);
            question.stay?.();
          }}
        />
      ) : (
        blocker.status === "blocked" && (
          <UnsavedChangesDialog
            key="section"
            leaving="section"
            forms={held()}
            leave={blocker.proceed}
            stay={blocker.reset}
          />
        )
      )}
    </UnsavedFormsContext.Provider>
  );
}

function UnsavedChangesDialog({
  leaving,
  forms,
  leave,
  stay,
}: {
  leaving: Leaving | "section";
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
    <ModalFrame
      busy={saving}
      close={stay}
      className="w-full max-w-md rounded-panel border border-border-strong bg-surface-raised p-6 shadow-lg"
    >
      <Dialog role="alertdialog" className="space-y-4 outline-none">
        <Heading slot="title" className="font-display text-xl font-semibold">
          <Trans>Save your changes?</Trans>
        </Heading>
        <p className="text-sm text-fg-muted">
          {leaving === "window" ? (
            <Trans>
              These changes aren't saved yet. Closing Caskade without saving loses them.
            </Trans>
          ) : leaving === "account" ? (
            <Trans>
              These changes aren't saved yet. Switching accounts without saving loses them.
            </Trans>
          ) : leaving === "mode" ? (
            <Trans>
              These changes aren't saved yet. Switching modes without saving loses them.
            </Trans>
          ) : (
            <Trans>
              These changes aren't saved yet. Leaving this section without saving loses them.
            </Trans>
          )}
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
    </ModalFrame>
  );
}
