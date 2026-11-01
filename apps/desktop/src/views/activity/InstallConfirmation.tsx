import { Button } from "@caskade/ui";
import type { MessageDescriptor } from "@lingui/core";
import { msg } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { useMutation } from "@tanstack/react-query";
import { Dialog, Heading, Modal, ModalOverlay } from "react-aria-components";
import type { DropPlan, InstallRequest } from "../../api/installRequests";

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
  const action = useMutation({
    mutationFn: (accept: boolean) =>
      accept ? confirm(request.requestId) : dismiss(request.requestId),
  });
  return (
    <ModalOverlay
      isOpen
      isDismissable={!action.isPending}
      onOpenChange={(open) => {
        if (!open && !action.isPending) action.mutate(false);
      }}
      className="fixed inset-0 z-50 grid place-items-center bg-surface-base/80 p-4 backdrop-blur-sm"
    >
      <Modal className="w-full max-w-xl rounded-panel border border-border-strong bg-surface-raised p-6">
        <Dialog
          data-cs="install-confirmation"
          data-focus-scope="install-confirmation"
          className="outline-none"
        >
          <Heading slot="title" className="font-display text-xl font-semibold">
            {title ? t`Install ${title}?` : t`Install this game?`}
          </Heading>
          {request.plan ? (
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
                Nothing will be downloaded until you confirm. If the download is an installer
                program, Caskade asks again before running it, unless Settings → Installer allows
                downloaded installers to run.
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
              {request.plan
                ? t({ message: "Install", context: "button", comment: "Starts an install" })
                : t`Confirm installation`}
            </Button>
          </div>
        </Dialog>
      </Modal>
    </ModalOverlay>
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
