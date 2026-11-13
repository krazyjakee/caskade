import { Button } from "@caskade/ui";
import type { MessageDescriptor } from "@lingui/core";
import { msg } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { useMutation } from "@tanstack/react-query";
import { Dialog, Heading } from "react-aria-components";
import type { DropPlan, InstallRequest, PeerSend } from "../../api/installRequests";
import { formatSize } from "../../api/presentation";
import { ModalFrame } from "../../components/ModalFrame";
import { ShareInfoDetails } from "../../components/shares/ShareInfoDetails";

export type { InstallRequest } from "../../api/installRequests";

/** External requests are inert until this explicit confirmation calls the
 * backend request-ID endpoint. Closing dismisses, never starts a download. */
export function InstallConfirmation({
  request,
  confirm,
  dismiss,
}: {
  request: InstallRequest;
  confirm: (requestId: string) => Promise<void>;
  dismiss: (requestId: string) => Promise<void>;
}) {
  const { t } = useLingui();
  const title = request.plan?.title;
  const shared = request.share?.title;
  const action = useMutation({
    mutationFn: (accept: boolean) =>
      accept ? confirm(request.requestId) : dismiss(request.requestId),
  });
  return (
    <ModalFrame
      busy={action.isPending}
      close={() => action.mutate(false)}
      className="w-full max-w-xl rounded-panel border border-border-strong bg-surface-raised p-6"
    >
      <Dialog
        data-cs="install-confirmation"
        data-focus-scope="install-confirmation"
        className="outline-none"
      >
        {request.share ? (
          <div data-part="share">
            <Heading slot="title" className="font-display text-xl font-semibold">
              {t`Receive ${shared}?`}
            </Heading>
            <div className="mt-3">
              <ShareInfoDetails info={request.share} />
            </div>
            <p className="mt-3 text-sm text-fg-muted">
              <Trans>
                It comes straight from their computer and installs like any other game; nothing is
                written until you say so.
              </Trans>
            </p>
          </div>
        ) : request.peer ? (
          <PeerDetails peer={request.peer} />
        ) : (
          <Heading slot="title" className="font-display text-xl font-semibold">
            {title ? t`Install ${title}?` : t`Install this game?`}
          </Heading>
        )}
        {request.peer || request.share ? null : request.plan ? (
          <PlanDetails plan={request.plan} />
        ) : (
          <>
            <p className="mt-3 text-sm text-fg-muted">
              {request.kind === "url"
                ? t`An external link asked Caskade to install from this URL. Confirm only if you trust its source.`
                : t`An external link asked Caskade to install from this path. Confirm only if you trust its source.`}
            </p>
            <p
              data-part="source"
              className="mt-4 max-h-40 overflow-auto break-all rounded-control border border-border p-3 font-mono text-xs"
            >
              {request.value}
            </p>
          </>
        )}
        {request.kind === "url" && (
          <p className="mt-3 text-sm text-fg-muted">
            <Trans>
              Nothing will be downloaded until you confirm. If the download is an installer program,
              Caskade runs it, unless Settings → Installer asks first for downloaded installers.
            </Trans>
          </p>
        )}
        {action.isError && (
          <p role="alert" className="mt-3 text-sm text-danger">
            {action.error.message}
          </p>
        )}
        <div className="mt-6 flex justify-end gap-2">
          <Button
            data-spatial="focusable"
            autoFocus
            isDisabled={action.isPending}
            onPress={() => action.mutate(false)}
          >
            <Trans>Dismiss</Trans>
          </Button>
          <Button
            data-spatial="focusable"
            variant="primary"
            isDisabled={action.isPending}
            onPress={() => action.mutate(true)}
          >
            {request.share?.contents.kind === "collection"
              ? t`Add to my collections`
              : request.share
                ? t({ message: "Receive", context: "button" })
                : request.peer
                  ? t`Copy here`
                  : request.plan
                    ? t({ message: "Install", context: "button", comment: "Starts an install" })
                    : t`Confirm installation`}
          </Button>
        </div>
      </Dialog>
    </ModalFrame>
  );
}

/** A game another of the user's devices sends here (#235): nothing is
 * written until it's confirmed. */
function PeerDetails({ peer }: { peer: PeerSend }) {
  const device = peer.deviceName;
  const title = peer.title;
  const size = peer.sizeBytes === null ? null : formatSize(peer.sizeBytes);
  return (
    <div data-part="peer">
      <Heading slot="title" className="font-display text-xl font-semibold">
        <Trans>
          Copy {title} from {device}?
        </Trans>
      </Heading>
      <p className="mt-3 text-sm text-fg-muted">
        {size ? (
          <Trans>
            {device} sends {title} ({size}) to this computer. It's copied straight from {device} and
            installed like any other game; nothing is written until you say so.
          </Trans>
        ) : (
          <Trans>
            {device} sends {title} to this computer. It's copied straight from {device} and{" "}
            installed like any other game; nothing is written until you say so.
          </Trans>
        )}
      </p>
    </div>
  );
}

const planSummaries: Record<DropPlan["kind"], MessageDescriptor> = {
  shortcut: msg`Caskade installs what this shortcut opens, with the shortcut's options.`,
  executable: msg`Caskade installs the game folder this program is in and launches it from there.`,
  folder: msg`Caskade installs this folder and picks the game's executable.`,
  file: msg`Caskade works out how to install this file: an installer runs, an archive is unpacked.`,
};

/** What a dropped file or folder installs as, worked out by the core. */
function PlanDetails({ plan }: { plan: DropPlan }) {
  const { t } = useLingui();
  const rows = [
    plan.dropped !== plan.input &&
      plan.dropped && {
        label: t({ message: "Dropped", comment: "The file or folder dropped on Caskade" }),
        value: plan.dropped,
      },
    {
      label: t({ message: "Installs", comment: "The file or folder an install request installs" }),
      value: plan.input,
    },
    plan.launch && {
      label: t({ message: "Launches", comment: "The command an installed game launches with" }),
      value: [plan.launch.path, ...plan.launch.args].join(" "),
    },
  ].filter((row): row is { label: string; value: string } => Boolean(row));
  return (
    <>
      <p className="mt-3 text-sm text-fg-muted">{t(planSummaries[plan.kind])}</p>
      <dl
        data-part="plan"
        className="mt-4 grid max-h-48 grid-cols-[auto_1fr] gap-x-3 gap-y-2 overflow-auto rounded-control border border-border p-3 text-xs"
      >
        {rows.map((row) => (
          <div key={row.label} className="contents">
            <dt className="text-fg-subtle">{row.label}</dt>
            <dd className="break-all font-mono">{row.value}</dd>
          </div>
        ))}
      </dl>
    </>
  );
}
