/** The button-hint bar (#148): what each button does where focus is now,
 * in the glyphs of the controller in hand (keys when no pad is in use),
 * and the clock. */
import { useLingui } from "@lingui/react/macro";
import { useQuery } from "@tanstack/react-query";
import { useContext, useEffect, useState } from "react";
import { formatTime, type TimeFormat } from "../../api/presentation";
import { useActiveBindings } from "../../input";
import { useOptionalInput } from "../../input/InputProvider";
import { useLocale } from "../../locale/LocaleProvider";
import { DefaultSettingsApi, settingsKeys } from "../../views/settings/useSettings";
import { faceTone, glyph, type HintFamily, hintOrder } from "./glyphs";

/** The pad family last used, or the keyboard. */
export function useHintFamily(): HintFamily {
  const input = useOptionalInput();
  return input?.source === "pad" ? input.glyphs : "keyboard";
}

/** Settings → General → Time format: 12 or 24 hours, or the language's
 * own clock (`system`, and until the settings answer). */
function useTimeFormat(): TimeFormat {
  const api = useContext(DefaultSettingsApi);
  const { data } = useQuery({
    queryKey: settingsKeys.section("core.general"),
    queryFn: () => api.get("core.general"),
    staleTime: 30_000,
    retry: false,
  });
  const format = data?.value.timeFormat;
  return format === "12h" || format === "24h" ? format : "system";
}

/** The time in the language's 12 or 24 hours, unless the Time format
 * setting says which. */
function Clock() {
  // Re-renders in a new language.
  useLocale();
  const format = useTimeFormat();
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    // Ticks on the minute.
    const wait = 60_000 - (Date.now() % 60_000);
    let interval: ReturnType<typeof setInterval> | undefined;
    const first = setTimeout(() => {
      setNow(new Date());
      interval = setInterval(() => setNow(new Date()), 60_000);
    }, wait);
    return () => {
      clearTimeout(first);
      clearInterval(interval);
    };
  }, []);
  return (
    <time
      data-part="clock"
      dateTime={now.toISOString()}
      className="font-mono text-sm text-fg-muted tabular-nums"
    >
      {formatTime(now, { format })}
    </time>
  );
}

export function HintBar() {
  const { t } = useLingui();
  const family = useHintFamily();
  const active = useActiveBindings();
  const hints = hintOrder.flatMap((action) => {
    const binding = active.find((candidate) => candidate.action === action);
    const label = glyph(action, family);
    return binding && label ? [{ action, label, text: binding.label }] : [];
  });
  return (
    <footer
      data-cs="hint-bar"
      data-glyphs={family}
      className="relative z-10 flex h-(--cs-hintbar-height) shrink-0 items-center gap-6 border-t border-border bg-surface-raised/90 px-8"
    >
      <ul
        aria-label={t({
          message: "Controls",
          comment: "The console hint bar's list of buttons and what they do",
        })}
        className="flex min-w-0 flex-1 items-center gap-6 overflow-hidden"
      >
        {hints.map(({ action, label, text }) => (
          <li
            key={action}
            data-part="hint"
            data-action={action}
            className="flex shrink-0 items-center gap-2 text-sm"
          >
            <kbd
              data-part="glyph"
              data-tone={faceTone[action]}
              className="grid h-7 min-w-7 place-items-center rounded-full border-2 border-border-strong px-1.5 font-mono text-xs font-semibold text-fg data-[tone=accent]:border-accent-alt data-[tone=accent]:text-accent-alt data-[tone=danger]:border-danger data-[tone=danger]:text-danger data-[tone=installed]:border-status-installed data-[tone=installed]:text-status-installed data-[tone=warning]:border-warning data-[tone=warning]:text-warning"
            >
              {label}
            </kbd>
            <span data-part="label" className="text-fg-muted">
              {text}
            </span>
          </li>
        ))}
      </ul>
      <Clock />
    </footer>
  );
}
