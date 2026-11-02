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
import { formatBytes, formatDateTime } from "../../api/presentation";
import type { SaveVersion } from "../../api/saves";
import { useSaveDeviceName } from "./devices";

const option =
  "block cursor-pointer rounded-control border border-border p-3 outline-none data-selected:border-accent-alt data-selected:bg-accent/10 data-focus-visible:focus-ring data-disabled:cursor-default data-disabled:opacity-50";

/** Both devices played since they last agreed (#239): which save to play
 * with. The others stay as versions. Newest first; this device's own is
 * chosen to begin with. */
export function SaveConflictDialog({
  versions,
  busy,
  error,
  close,
  choose,
}: {
  versions: SaveVersion[];
  busy: boolean;
  error?: string;
  close: () => void;
  choose: (versionId: string) => void;
}) {
  const { t } = useLingui();
  const deviceName = useSaveDeviceName();
  const [chosen, setChosen] = useState<string | null>(
    (versions.find((each) => each.local) ?? versions[0])?.id ?? null,
  );
  return (
    <ModalOverlay
      isOpen
      onOpenChange={(open) => {
        if (!open && !busy) close();
      }}
      isDismissable={!busy}
      className="fixed inset-0 z-50 grid place-items-center bg-surface-base/80 p-4 backdrop-blur-sm"
    >
      <Modal className="max-h-[85vh] w-full max-w-lg overflow-auto rounded-panel border border-border-strong bg-surface-raised p-6 shadow-lg">
        <Dialog data-cs="save-conflict" className="outline-none">
          <Heading slot="title" className="font-display text-xl font-semibold">
            {t`Choose which save to play with`}
          </Heading>
          <p className="mt-2 text-sm text-fg-muted">
            <Trans>
              This game was played on more than one device since they last shared saves. The save
              you don't choose is kept as a version.
            </Trans>
          </p>
          <RadioGroup
            data-part="versions"
            value={chosen}
            onChange={setChosen}
            isDisabled={busy}
            className="mt-5 space-y-2"
          >
            <Label className="sr-only">{t`Saves`}</Label>
            {versions.map((version) => (
              <Radio
                key={version.id}
                value={version.id}
                data-spatial="focusable"
                data-focus-default={version.id === chosen ? "" : undefined}
                className={option}
              >
                <span className="text-sm font-medium">{deviceName(version)}</span>
                <span className="mt-1 block text-xs text-fg-muted">
                  {formatDateTime(version.createdAtMs)} · {formatBytes(version.sizeBytes)}
                </span>
              </Radio>
            ))}
          </RadioGroup>
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
              isDisabled={chosen === null || busy}
              onPress={() => chosen && choose(chosen)}
            >
              {busy ? t`Starting…` : t`Play with this save`}
            </Button>
          </div>
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}
