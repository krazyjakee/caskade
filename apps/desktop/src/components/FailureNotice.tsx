import { Button, ErrorNotice } from "@caskade/ui";
import { useLingui } from "@lingui/react/macro";
import { RotateCcw } from "lucide-react";
import type { ReactNode, Ref } from "react";
import { hintLabels, toFailure } from "../api/reply";

/** Any error as an [`ErrorNotice`] (#160): the core's summary, its recovery
 * hints in words, and its redacted details and log reference behind Show
 * details. `onRetry` adds Try again, the notice's first stop for a pad. */
export function FailureNotice({
  error,
  title,
  onRetry,
  retryLabel,
  retryRef,
  actions,
  variant = "block",
  className,
}: {
  error: unknown;
  title?: ReactNode;
  onRetry?: () => void;
  retryLabel?: string;
  /** For `useDefaultFocus`, when the notice stands in for a whole view. */
  retryRef?: Ref<HTMLButtonElement>;
  actions?: ReactNode;
  variant?: "block" | "inline";
  className?: string;
}) {
  const { t } = useLingui();
  const failure = toFailure(error);
  const hints = failure.hints
    // Try again says it already.
    .filter((hint) => !(onRetry && hint === "retry"))
    .map((hint) => hintLabels[hint])
    .filter((hint): hint is string => Boolean(hint));
  return (
    <ErrorNotice
      title={title}
      summary={failure.message}
      hints={hints}
      detail={failure.detail}
      reference={failure.reference}
      variant={variant}
      className={className}
      actions={
        onRetry || actions ? (
          <>
            {onRetry && (
              <Button
                ref={retryRef}
                size="sm"
                icon={RotateCcw}
                data-focus-default={retryRef ? true : undefined}
                onPress={onRetry}
              >
                {retryLabel ?? t`Try again`}
              </Button>
            )}
            {actions}
          </>
        ) : undefined
      }
    />
  );
}
