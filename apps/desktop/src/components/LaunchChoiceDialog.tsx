import { Button } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { useState } from "react";
import { Dialog, Heading, Label, Radio, RadioGroup } from "react-aria-components";
import type { LaunchChoice, LaunchPrompt } from "../api/types";
import { ModalFrame } from "./ModalFrame";

const option =
  "block cursor-pointer rounded-control border border-border p-3 outline-none data-selected:border-accent-alt data-selected:bg-accent/10 data-focus-visible:focus-ring data-disabled:cursor-default data-disabled:opacity-50";

/** Ask-at-launch and the disc picker: the launch asked
 * for a play action (#178), a profile, a disc or both; continuing launches
 * again with the answers. */
export function LaunchChoiceDialog({
  prompt,
  busy,
  error,
  close,
  choose,
}: {
  prompt: LaunchPrompt;
  busy: boolean;
  error?: string;
  close: () => void;
  choose: (choice: LaunchChoice) => void;
}) {
  const { t } = useLingui();
  const [profile, setProfile] = useState<string | null>(null);
  const [disc, setDisc] = useState<string | null>(null);
  const actions = prompt.actions ?? [];
  const [action, setAction] = useState<string | null>(
    actions.find((each) => each.default)?.id ?? null,
  );
  const key = (p: LaunchPrompt["profiles"][number]) => `${p.emulatorId}/${p.profileId}`;
  const chosen = prompt.profiles.find((p) => key(p) === profile);
  const ready =
    (actions.length === 0 || action !== null) &&
    (prompt.profiles.length === 0 || chosen !== undefined) &&
    (prompt.discs.length === 0 || disc !== null);
  return (
    <ModalFrame
      busy={busy}
      close={close}
      className="max-h-[85vh] w-full max-w-lg overflow-auto rounded-panel border border-border-strong bg-surface-raised p-6 shadow-lg"
    >
      <Dialog data-cs="launch-choice" className="outline-none">
        <Heading slot="title" className="font-display text-xl font-semibold">
          {actions.length > 0 || prompt.profiles.length > 0
            ? t`Choose how to play`
            : t`Choose a disc`}
        </Heading>
        {actions.length > 0 && (
          <RadioGroup
            data-part="actions"
            value={action}
            onChange={setAction}
            isDisabled={busy}
            className="mt-5 space-y-2"
          >
            <Label className="text-sm font-semibold">
              {t({ message: "Start with", context: "launch choice" })}
            </Label>
            {actions.map((each) => (
              <Radio
                key={each.id}
                value={each.id}
                data-spatial="focusable"
                data-focus-default={each.default ? "" : undefined}
                className={option}
              >
                <span className="text-sm font-medium">{each.name}</span>
                {each.default && (
                  <span className="mt-1 block text-xs text-fg-muted">
                    {t({ message: "Default", context: "launch action" })}
                  </span>
                )}
              </Radio>
            ))}
          </RadioGroup>
        )}
        {prompt.profiles.length > 0 && (
          <RadioGroup
            data-part="profiles"
            value={profile}
            onChange={setProfile}
            isDisabled={busy}
            className="mt-5 space-y-2"
          >
            <Label className="text-sm font-semibold">
              {t({ message: "Emulator", context: "launch choice" })}
            </Label>
            {prompt.profiles.map((p) => (
              <Radio
                key={key(p)}
                value={key(p)}
                isDisabled={!p.installed}
                data-spatial="focusable"
                className={option}
              >
                <span className="text-sm font-medium">
                  {p.emulatorName} · {p.profileName}
                </span>
                {!p.installed && (
                  <span className="mt-1 block text-xs text-fg-muted">{t`Not installed`}</span>
                )}
              </Radio>
            ))}
          </RadioGroup>
        )}
        {prompt.discs.length > 0 && (
          <RadioGroup
            data-part="discs"
            value={disc}
            onChange={setDisc}
            isDisabled={busy}
            className="mt-5 space-y-2"
          >
            <Label className="text-sm font-semibold">
              {t({ message: "Disc", context: "launch choice" })}
            </Label>
            {prompt.discs.map((d) => (
              <Radio
                key={d.index}
                value={String(d.index)}
                data-spatial="focusable"
                className={option}
              >
                <span className="break-all text-sm font-medium">{d.name}</span>
              </Radio>
            ))}
          </RadioGroup>
        )}
        {error && (
          <p role="alert" className="mt-3 text-sm text-danger">
            {error}
          </p>
        )}
        <div className="mt-6 flex justify-end gap-2">
          <Button data-spatial="focusable" onPress={close} isDisabled={busy}>
            <Trans>Cancel</Trans>
          </Button>
          <Button
            data-spatial="focusable"
            variant="primary"
            isDisabled={!ready || busy}
            onPress={() =>
              choose({
                profile: chosen
                  ? { emulatorId: chosen.emulatorId, profileId: chosen.profileId }
                  : null,
                disc: disc === null ? null : Number(disc),
                ...(actions.length > 0 ? { action } : {}),
              })
            }
          >
            {busy ? t`Starting…` : t({ message: "Play", context: "game action" })}
          </Button>
        </div>
      </Dialog>
    </ModalFrame>
  );
}
