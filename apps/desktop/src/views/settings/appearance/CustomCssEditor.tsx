/** Appearance → Custom CSS (#142): the user's rules in the `cs.user` layer,
 * saved through the settings service and applied without a reload. A
 * switch turns them off without losing them, for a look that broke. */
import { Button, SettingRow, Switch } from "@caskade/ui";
import { msg } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { BookOpen, PowerOff } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { TextArea, TextField } from "react-aria-components";
import { formatNumber } from "../../../api/presentation";
import {
  type CssProblem,
  checkUserCss,
  problemText,
  USER_CSS_MAX,
} from "../../../appearance/userCss";
import { UnsavedHint, useUnsavedChanges } from "../unsavedChanges";

/** The theming contract's documentation (`docs/theming/`). */
const THEMING_DOCS = "https://github.com/krazyjakee/caskade/blob/master/docs/theming/";

const docs = [
  {
    href: `${THEMING_DOCS}anatomy.md`,
    label: msg({ message: "Selectors", context: "theming docs" }),
  },
  { href: `${THEMING_DOCS}tokens.md`, label: msg({ message: "Tokens", context: "theming docs" }) },
  { href: `${THEMING_DOCS}README.md`, label: msg`Theming guide` },
] as const;

export function CustomCssEditor({
  css,
  enabled,
  safeMode,
  saving,
  onSave,
  onEnabledChange,
}: {
  css: string;
  enabled: boolean;
  safeMode: boolean;
  saving: boolean;
  onSave: (css: string) => void;
  onEnabledChange: (enabled: boolean) => void;
}) {
  const { t } = useLingui();
  const [draft, setDraft] = useState<string | null>(null);
  const [problems, setProblems] = useState<CssProblem[]>([]);
  const gutter = useRef<HTMLDivElement>(null);
  const text = draft ?? css;
  const tooLong = text.length > USER_CSS_MAX;
  const lineNumbers = Array.from({ length: text.split("\n").length }, (_, line) => line + 1);
  // Once the saved value is what was typed, the draft is done.
  useEffect(() => {
    if (draft !== null && draft === css) setDraft(null);
  }, [css, draft]);
  const dirty = draft !== null && draft !== css;
  /** Saves the draft unless it has problems, which show under the editor. */
  const save = () => {
    const found = checkUserCss(text);
    setProblems(found);
    if (found.length > 0 || tooLong) return false;
    onSave(text);
    return true;
  };
  const revert = () => {
    setDraft(null);
    setProblems([]);
  };
  // The CSS waits for Save CSS (#43): leaving the section asks first.
  useUnsavedChanges(dirty, { label: t`Custom CSS`, save, discard: revert });
  return (
    <div className="flex flex-col gap-3">
      <SettingRow
        icon={PowerOff}
        label={t`Disable custom CSS`}
        description={t`Keeps your CSS but stops applying it, to recover from a broken look.`}
      >
        <Switch isSelected={!enabled} onChange={(off) => onEnabledChange(!off)} />
      </SettingRow>
      {safeMode && (
        <p
          role="status"
          className="rounded-control border border-warning/40 bg-warning/10 px-3 py-2 text-sm"
        >
          <Trans>
            Safe mode is on: your custom CSS isn't applied until Caskade starts normally.
          </Trans>
        </p>
      )}
      <div className="flex overflow-hidden rounded-control border border-border-strong bg-surface-base/60 font-mono text-xs leading-5 focus-within:border-accent-alt/60">
        <div
          ref={gutter}
          aria-hidden="true"
          className="shrink-0 overflow-hidden border-r border-border py-2 pr-2 pl-3 text-right text-fg-subtle select-none"
        >
          {lineNumbers.map((number) => (
            <div key={number}>{number}</div>
          ))}
        </div>
        <TextField aria-label={t`Custom CSS`} value={text} onChange={setDraft} className="flex-1">
          <TextArea
            wrap="off"
            spellCheck={false}
            rows={14}
            placeholder={
              '[data-cs="game-card"] [data-part="title"] {\n  color: var(--cs-color-accent);\n}'
            }
            onScroll={(event) => {
              if (gutter.current) gutter.current.scrollTop = event.currentTarget.scrollTop;
            }}
            className="block h-72 w-full resize-y bg-transparent py-2 pr-3 pl-2 text-fg outline-none placeholder:text-fg-subtle"
          />
        </TextField>
      </div>
      <div className="flex flex-wrap items-center gap-2">
        <span className={`mr-auto font-mono text-xs ${tooLong ? "text-danger" : "text-fg-subtle"}`}>
          {t`${{ length: formatNumber(text.length) }} / ${{ max: formatNumber(USER_CSS_MAX) }} characters`}
        </span>
        <UnsavedHint dirty={dirty} />
        <Button variant="ghost" size="sm" isDisabled={draft === null} onPress={revert}>
          <Trans>Revert</Trans>
        </Button>
        <Button
          variant="primary"
          size="sm"
          isDisabled={draft === null || saving || tooLong}
          onPress={save}
        >
          <Trans>Save CSS</Trans>
        </Button>
      </div>
      {problems.length > 0 && (
        <div role="alert" className="text-xs text-danger">
          <p className="font-medium">
            <Trans>Custom CSS wasn't saved:</Trans>
          </p>
          <p className="font-mono whitespace-pre-line">{problemText(problems)}</p>
        </div>
      )}
      <p className="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-fg-muted">
        <BookOpen className="size-3.5" />
        <Trans>Your rules beat the theme and every component. Style through the contract:</Trans>
        {docs.map((doc) => (
          <a
            key={doc.href}
            href={doc.href}
            target="_blank"
            rel="noreferrer"
            className="rounded-sm text-accent-alt underline-offset-2 hover:underline focus-visible:focus-ring"
          >
            {t(doc.label)}
          </a>
        ))}
      </p>
    </div>
  );
}
