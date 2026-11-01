import { Trans, useLingui } from "@lingui/react/macro";
import { CircleAlert, CircleX } from "lucide-react";
import { type ReactNode, useId, useState } from "react";
import { Button as AriaButton } from "react-aria-components";

export interface ErrorNoticeProps {
  /** What couldn't be done, e.g. "Couldn't load your library". */
  title?: ReactNode;
  /** What failed and, when known, why: the error's summary. */
  summary: ReactNode;
  /** What the user can do, one sentence each. */
  hints?: readonly string[];
  /** Technical details, redacted; behind Show details. */
  detail?: string | null;
  /** The reference logged with the details. */
  reference?: string | null;
  /** Recovery controls, e.g. Try again. */
  actions?: ReactNode;
  /** `block` fills a view or panel; `inline` sits under a control. */
  variant?: "block" | "inline";
  /** `warning` while Caskade is still recovering on its own. */
  tone?: "danger" | "warning";
  /** Starts with the details shown. */
  defaultExpanded?: boolean;
  className?: string;
}

/** A failure, shown the same way everywhere (#160): a plain summary, what to
 * do, and the technical details and log reference one press away. The
 * details toggle is a button, so a keyboard or a pad reaches it. */
export function ErrorNotice({
  title,
  summary,
  hints = [],
  detail,
  reference,
  actions,
  variant = "block",
  tone = "danger",
  defaultExpanded = false,
  className = "",
}: ErrorNoticeProps) {
  const { t } = useLingui();
  const [expanded, setExpanded] = useState(defaultExpanded);
  const id = useId();
  const hasDetails = Boolean(detail || reference);
  const Icon = tone === "warning" ? CircleAlert : CircleX;
  return (
    <div
      data-cs="error-notice"
      data-variant={variant}
      data-tone={tone}
      data-expanded={hasDetails ? String(expanded) : undefined}
      role="alert"
      className={`flex gap-3 text-sm ${
        variant === "block" ? "rounded-panel border border-border bg-surface-raised/60 p-5" : ""
      } ${className}`}
    >
      <Icon
        data-part="icon"
        aria-hidden
        className={`mt-0.5 size-4 shrink-0 ${tone === "warning" ? "text-warning" : "text-danger"}`}
      />
      <div className="min-w-0 flex-1">
        {title && (
          <p data-part="title" className="font-medium text-fg">
            {title}
          </p>
        )}
        <p
          data-part="summary"
          className={`break-words ${title ? "mt-1 text-fg-muted" : tone === "warning" ? "text-warning" : "text-danger"}`}
        >
          {summary}
        </p>
        {hints.length > 0 && (
          <ul data-part="hints" className="mt-2 list-disc space-y-0.5 pl-5 text-fg-muted">
            {hints.map((hint) => (
              <li key={hint}>{hint}</li>
            ))}
          </ul>
        )}
        {(actions || hasDetails) && (
          <div data-part="actions" className="mt-3 flex flex-wrap items-center gap-2">
            {actions}
            {hasDetails && (
              <AriaButton
                data-part="details-toggle"
                aria-expanded={expanded}
                aria-controls={`${id}-details`}
                onPress={() => setExpanded((open) => !open)}
                className="rounded-control px-2 py-1 text-xs text-fg-muted underline-offset-2 data-focus-visible:focus-ring data-hovered:text-fg data-hovered:underline"
              >
                {expanded
                  ? t({ message: "Hide details", comment: "Hides an error's technical details" })
                  : t({ message: "Show details", comment: "Shows an error's technical details" })}
              </AriaButton>
            )}
          </div>
        )}
        {hasDetails && expanded && (
          <div
            id={`${id}-details`}
            data-part="details"
            className="mt-2 rounded-control border border-border bg-surface-base/60 p-3 font-mono text-xs text-fg-muted"
          >
            {detail && <p className="break-words whitespace-pre-wrap select-text">{detail}</p>}
            {reference && (
              <p
                data-part="reference"
                className={detail ? "mt-2 text-fg-subtle" : "text-fg-subtle"}
              >
                <Trans comment="The id an error's details are logged under">
                  Reference: {reference}
                </Trans>
              </p>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
