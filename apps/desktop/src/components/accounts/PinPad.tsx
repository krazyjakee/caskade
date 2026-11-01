/** PIN entry (#207): a field for keys and a number pad for a controller. A
 * pad's A types the focused digit, B deletes (or leaves when there is
 * nothing to delete), Start confirms. Shared by sign-in, and by the age
 * gate and admin approval. The PIN lives only in this component's state
 * until it goes to the core. */
import { useLingui } from "@lingui/react/macro";
import { Check, Delete } from "lucide-react";
import { useEffect, useId, useRef, useState } from "react";
import { Button } from "react-aria-components";
import type { PinCheck } from "../../api/accounts";
import { validPin } from "../../api/accounts";
import { errorMessage } from "../../api/reply";
import { useDefaultFocus, useInputAction } from "../../input";
import { useOptionalInput } from "../../input/InputProvider";
import { focus } from "../../input/spatial";

type Status = "ready" | "wrong" | "locked";

const DIGITS = ["1", "2", "3", "4", "5", "6", "7", "8", "9"] as const;

/** "0:27", "12:05". */
function wait(ms: number): string {
  const seconds = Math.max(0, Math.ceil(ms / 1000));
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
}

/** The time now, rendering again every second while `until` is ahead. The
 * clock is read as the view renders: a time kept from the last tick would
 * be up to a second old when a wait starts, and a 30 s wait would show 0:31. */
function useCountdown(until: number | null): number {
  const [, setTick] = useState(0);
  useEffect(() => {
    if (until === null) return;
    const timer = setInterval(() => setTick((tick) => tick + 1), 1000);
    return () => clearInterval(timer);
  }, [until]);
  return Date.now();
}

const key =
  "grid h-14 place-items-center rounded-control border border-border-strong bg-surface-overlay/70 font-display text-xl font-semibold text-fg outline-none transition-[background-color,border-color,scale] duration-(--cs-motion-duration-fast) data-focus-visible:focus-ring data-hovered:border-accent-alt/50 data-hovered:bg-surface-hover data-pressed:scale-[0.96] console:h-16";

export function PinPad({
  title,
  check,
  onMatched,
  onCancel,
  lockedUntilMs = null,
}: {
  /** What the PIN is for: "Enter Ada's PIN". */
  title: string;
  /** Asks the core; the answer never says whose PIN it was. */
  check(pin: string): Promise<PinCheck>;
  onMatched(): void;
  /** B with nothing to delete, or Escape. */
  onCancel?(): void;
  /** When wrong PINs have already locked it. */
  lockedUntilMs?: number | null;
}) {
  const { t } = useLingui();
  const input = useOptionalInput();
  const root = useRef<HTMLDivElement>(null);
  const field = useRef<HTMLInputElement>(null);
  const first = useRef<HTMLButtonElement>(null);
  const [pin, setPin] = useState("");
  const [status, setStatus] = useState<Status>("ready");
  const [message, setMessage] = useState<string | null>(null);
  const [until, setUntil] = useState<number | null>(
    lockedUntilMs && lockedUntilMs > Date.now() ? lockedUntilMs : null,
  );
  const [attempt, setAttempt] = useState(0);
  const [pending, setPending] = useState(false);
  const now = useCountdown(until);
  const messageId = useId();
  const locked = until !== null && until > now;
  const shown: Status = locked ? "locked" : status === "locked" ? "ready" : status;

  // The core locked it (the last wrong PIN started a wait).
  useEffect(() => {
    if (lockedUntilMs && lockedUntilMs > Date.now()) setUntil(lockedUntilMs);
  }, [lockedUntilMs]);
  // The wait is over: the field takes a PIN again.
  useEffect(() => {
    if (until !== null && until <= now) {
      setUntil(null);
      setMessage(null);
    }
  }, [until, now]);

  // A controller starts on the number pad; keys start in the field.
  useDefaultFocus(first);
  // biome-ignore lint/correctness/useExhaustiveDependencies: only when it opens.
  useEffect(() => {
    if (!input?.active) field.current?.focus();
    // A dialog keeps focus inside, so when one step's pad makes way for the
    // next, focus lands on the new field: a controller still starts on the keys.
    else if (first.current && document.activeElement === field.current) {
      focus(first.current, true);
      input.entered.current = first.current;
    }
  }, []);

  const type = (digit: string) => {
    if (locked || pending) return;
    setStatus("ready");
    setMessage(null);
    setPin((current) => (current.length < 8 ? current + digit : current));
  };
  const erase = () => setPin((current) => current.slice(0, -1));
  const submit = async () => {
    if (locked || pending) return;
    if (!validPin(pin)) {
      setMessage(t`A PIN is 4 to 8 digits.`);
      return;
    }
    // A wrong PIN clears the field and draws it again, to shake it; the
    // keyboard stays in it.
    const inField = document.activeElement === field.current;
    const refocus = () => {
      if (inField) requestAnimationFrame(() => field.current?.focus());
    };
    setPending(true);
    try {
      const answer = await check(pin);
      if (answer.type === "matched") {
        setPin("");
        onMatched();
        return;
      }
      setPin("");
      setAttempt((count) => count + 1);
      if (answer.type === "locked") {
        setUntil(answer.untilMs);
        setStatus("locked");
        setMessage(null);
      } else {
        setStatus("wrong");
        setMessage(t`That PIN isn't right.`);
      }
      refocus();
    } catch (error) {
      setPin("");
      setStatus("wrong");
      setMessage(errorMessage(error));
      refocus();
    } finally {
      setPending(false);
    }
  };

  const deleteLabel = t({
    message: "Delete",
    context: "PIN pad",
    comment: "Deletes the last digit",
  });
  const confirmLabel = t({ message: "Confirm", context: "PIN pad" });
  useInputAction("confirm", () => false, {
    label: t({ message: "Type", context: "PIN pad", comment: "Types the focused digit" }),
    owner: root,
  });
  useInputAction(
    "back",
    () => {
      if (pin) {
        erase();
        return true;
      }
      if (!onCancel) return false;
      onCancel();
      return true;
    },
    {
      label: pin || !onCancel ? deleteLabel : t({ message: "Back", context: "PIN pad" }),
      owner: root,
    },
  );
  useInputAction("menu", () => void submit(), { label: confirmLabel, owner: root });

  return (
    // Digits typed anywhere on the pad go to the PIN; the field and the keys
    // are its controls.
    // biome-ignore lint/a11y/noStaticElementInteractions: see above.
    <div
      ref={root}
      data-cs="pin-pad"
      data-status={shown}
      onKeyDown={(event) => {
        if (event.target === field.current) return;
        if (/^\d$/.test(event.key)) {
          event.preventDefault();
          type(event.key);
        } else if (event.key === "Escape" && onCancel) {
          event.preventDefault();
          onCancel();
        }
      }}
      className="flex w-full max-w-xs flex-col items-center gap-4"
    >
      <h2 data-part="title" className="text-center font-display text-lg font-semibold">
        {title}
      </h2>
      <form
        className="w-full"
        onSubmit={(event) => {
          event.preventDefault();
          void submit();
        }}
      >
        <input
          key={attempt}
          ref={field}
          data-part="field"
          type="password"
          inputMode="numeric"
          autoComplete="off"
          aria-label={title}
          aria-invalid={shown === "wrong" || undefined}
          aria-describedby={message || locked ? messageId : undefined}
          readOnly={locked}
          maxLength={8}
          value={pin}
          onChange={(event) => {
            if (locked) return;
            setStatus("ready");
            setMessage(null);
            setPin(event.target.value.replace(/\D/g, "").slice(0, 8));
          }}
          onKeyDown={(event) => {
            if (event.key === "Escape" && onCancel) {
              event.preventDefault();
              onCancel();
            }
          }}
          className={`h-14 w-full rounded-control border border-border-strong bg-surface-base/60 text-center font-mono text-2xl tracking-[0.5em] text-fg outline-none focus-visible:focus-ring ${shown === "wrong" ? "animate-shake border-danger" : ""}`}
        />
      </form>
      <p
        id={messageId}
        data-part="message"
        role="status"
        hidden={!message && !locked}
        className={`min-h-5 text-center text-sm ${locked || shown === "wrong" ? "text-danger" : "text-fg-muted"}`}
      >
        {locked
          ? t`Too many wrong PINs. Try again in ${{ time: wait((until ?? now) - now) }}.`
          : message}
      </p>
      <div data-part="keys" className="grid w-full grid-cols-3 gap-2">
        {DIGITS.map((digit) => (
          <Button
            key={digit}
            ref={digit === "1" ? first : undefined}
            data-part="key"
            data-focus-default={digit === "1" || undefined}
            onPress={() => type(digit)}
            className={key}
          >
            {digit}
          </Button>
        ))}
        <Button data-part="delete" aria-label={deleteLabel} onPress={erase} className={key}>
          <Delete className="size-5" />
        </Button>
        <Button data-part="key" onPress={() => type("0")} className={key}>
          0
        </Button>
        <Button
          data-part="submit"
          aria-label={confirmLabel}
          isDisabled={pending}
          onPress={() => void submit()}
          className={`${key} bg-linear-to-r from-accent to-accent-strong text-accent-fg`}
        >
          <Check className="size-5" />
        </Button>
      </div>
    </div>
  );
}
