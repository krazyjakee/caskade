import { Button } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { useState } from "react";
import {
  Dialog,
  Heading,
  Label,
  Modal,
  ModalOverlay,
  Radio,
  RadioGroup,
} from "react-aria-components";
import type { JobQuestion } from "../../api/install";

/** The question ID keys this component, clearing a prior selection on change.
 * React Aria provides modal focus containment/restoration and radio arrow keys. */
export function QuestionDialog({
  question,
  busy,
  error,
  close,
  answer,
}: {
  question: JobQuestion;
  busy: boolean;
  error?: string;
  close: () => void;
  answer: (choiceId: string) => void;
}) {
  const { t } = useLingui();
  const [selected, setSelected] = useState<string | null>(null);
  return (
    <ModalOverlay
      isOpen
      onOpenChange={(open) => {
        if (!open && !busy) close();
      }}
      isDismissable={!busy}
      className="fixed inset-0 z-50 grid place-items-center bg-surface-base/80 p-4 backdrop-blur-sm"
    >
      <Modal className="max-h-[85vh] w-full max-w-2xl overflow-auto rounded-panel border border-border-strong bg-surface-raised p-6 shadow-lg">
        <Dialog
          data-cs="job-question"
          data-focus-scope="activity-question"
          className="outline-none"
        >
          <Heading slot="title" className="font-display text-xl font-semibold">
            {question.title}
          </Heading>
          <p className="mt-2 text-sm text-fg-muted">{question.message}</p>
          <RadioGroup
            value={selected}
            onChange={setSelected}
            isDisabled={busy || question.canAnswer === false}
            className="mt-5 space-y-2"
          >
            <Label className="text-sm font-semibold">
              {question.canAnswer === false ? t`Installation details` : t`Choose an option`}
            </Label>
            {question.choices.map((choice) => (
              <Radio
                key={choice.id}
                value={choice.id}
                data-spatial="focusable"
                className="block cursor-pointer rounded-control border border-border p-3 outline-none data-selected:border-accent-alt data-selected:bg-accent/10 data-focus-visible:focus-ring"
              >
                <span className="text-sm font-medium">{choice.label}</span>
                {choice.detail && (
                  <span className="mt-1 block break-all text-xs text-fg-muted">
                    {choice.detail}
                  </span>
                )}
              </Radio>
            ))}
          </RadioGroup>
          {question.choices.length === 0 && (
            <p className="mt-4 text-sm text-fg-muted">
              <Trans>
                No selectable options were supplied. Leave this job waiting or cancel it in
                Activity.
              </Trans>
            </p>
          )}
          {error && (
            <p role="alert" className="mt-3 text-sm text-danger">
              {error}
            </p>
          )}
          <div className="mt-6 flex justify-end gap-2">
            <Button data-spatial="focusable" onPress={close} isDisabled={busy}>
              <Trans>Not now</Trans>
            </Button>
            {question.canAnswer !== false && (
              <Button
                data-spatial="focusable"
                variant="primary"
                isDisabled={!selected || busy}
                onPress={() => {
                  if (selected) answer(selected);
                }}
              >
                {busy ? t`Submitting…` : t`Continue`}
              </Button>
            )}
          </div>
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}
