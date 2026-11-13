import { Button } from "@caskade/ui";
import type { MessageDescriptor } from "@lingui/core";
import { msg, plural } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { FolderSearch, HardDrive, RotateCcw, TimerOff } from "lucide-react";
import { useState } from "react";
import { Dialog, Disclosure, DisclosurePanel, Heading } from "react-aria-components";
import {
  type JobFailure,
  type JobRecovery,
  type JobSnapshot,
  mergeJobs,
  type RecoveryAction,
} from "../../api/install";
import { formatTime } from "../../api/presentation";
import { ModalFrame } from "../../components/ModalFrame";
import { usePathPicker } from "../../components/pathBrowser/PathPicker";
import { InstallRootPicker } from "../../components/storage/InstallRootPicker";
import { activityKey, useActivityStorage, useFollowJob, useInstallApi } from "./ActivityProvider";

const stageLabels: Record<JobFailure["stage"], MessageDescriptor> = {
  download: msg({ message: "Download", comment: "The stage a failed install stopped at" }),
  inspect: msg({
    message: "Reading the installer",
    comment: "The stage a failed install stopped at",
  }),
  install: msg({ message: "Installing", comment: "The stage a failed install stopped at" }),
  finalize: msg({ message: "Finishing", comment: "The stage a failed install stopped at" }),
  commit: msg({
    message: "Checking the install",
    comment: "The stage a failed install stopped at",
  }),
};

const actionLabels: Record<Exclude<RecoveryAction, "cancel">, MessageDescriptor> = {
  retry: msg`Try again`,
  redownload: msg`Download again`,
  locateSource: msg`Locate the installer…`,
  chooseDestination: msg`Choose another location…`,
};

const actionIcons = {
  retry: RotateCcw,
  redownload: RotateCcw,
  locateSource: FolderSearch,
  chooseDestination: HardDrive,
};

/** The parent folder of `path`, to start a picker in. */
function folderOf(path?: string) {
  if (!path) return null;
  const cut = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return cut > 0 ? path.slice(0, cut) : null;
}

/** Why a failed install or update stopped and what can be done about it
 * (#158): the stage, the file, a plain explanation, redacted details and the
 * recovery actions that suit the cause. A pad reaches every button. */
export function JobFailurePanel({
  job,
  failure,
  busy,
}: {
  job: JobSnapshot;
  failure: JobFailure;
  busy: boolean;
}) {
  const { t } = useLingui();
  const api = useInstallApi();
  const storageApi = useActivityStorage();
  const picker = usePathPicker();
  const client = useQueryClient();
  const follow = useFollowJob();
  const [choosingRoot, setChoosingRoot] = useState(false);
  const recover = useMutation({
    mutationFn: async (recovery: JobRecovery | "stopRetry") => {
      if (recovery === "stopRetry") {
        await api.cancel(job.id);
        return;
      }
      const next = await api.recover(job.id, recovery);
      client.setQueryData<JobSnapshot[]>(activityKey, (current) => mergeJobs(current, [next]));
      follow(job.id);
    },
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: activityKey });
    },
  });
  const locate = useMutation({
    mutationFn: async () => {
      const path = await picker.pick(
        { kind: "file", title: t`Locate the installer`, startAt: folderOf(failure.path) },
        async () => (await api.choosePaths("file"))[0] ?? null,
      );
      if (path) await recover.mutateAsync({ type: "locateSource", input: path });
    },
  });
  const pending = busy || recover.isPending || locate.isPending;
  const error = recover.error ?? locate.error;
  const actions = failure.actions.filter(
    (action): action is Exclude<RecoveryAction, "cancel"> =>
      action !== "cancel" &&
      (action !== "chooseDestination" || (storageApi !== undefined && job.kind === "install")),
  );
  const retryAt = failure.retryAtMs === undefined ? "" : formatTime(failure.retryAtMs);
  const run = (action: Exclude<RecoveryAction, "cancel">) => {
    recover.reset();
    locate.reset();
    if (action === "locateSource") locate.mutate();
    else if (action === "chooseDestination") setChoosingRoot(true);
    else recover.mutate({ type: action });
  };
  return (
    <section
      aria-label={t`What went wrong`}
      data-part="failure"
      data-kind={failure.kind}
      data-status={failure.retryAtMs ? "retrying" : "waiting"}
      className="mt-4 rounded-control border border-danger/40 p-3"
    >
      <p role="alert" className="text-sm text-danger">
        {failure.summary}
      </p>
      <p className="mt-1 font-mono text-xs text-fg-subtle">
        {t(stageLabels[failure.stage])}
        {failure.path ? ` · ${failure.path}` : ""}
      </p>
      {failure.retryAtMs !== undefined ? (
        <p className="mt-2 text-sm text-fg-muted">
          <Trans>Caskade will try again at {retryAt}.</Trans>
        </p>
      ) : (
        <p className="mt-2 text-sm text-fg-muted">
          {failure.transient
            ? t`Caskade isn't retrying this on its own now. Choose what to do next.`
            : t`This needs you: choose what to do next.`}
        </p>
      )}
      <div className="mt-3 flex flex-wrap gap-2">
        {failure.retryAtMs !== undefined && (
          <Button
            data-spatial="focusable"
            variant="ghost"
            isDisabled={pending}
            onPress={() => recover.mutate("stopRetry")}
          >
            <TimerOff className="size-4" /> <Trans>Don't retry automatically</Trans>
          </Button>
        )}
        {actions.map((action, index) => {
          const Icon = actionIcons[action];
          return (
            <Button
              key={action}
              data-spatial="focusable"
              variant={index === 0 ? "primary" : "ghost"}
              isDisabled={pending}
              onPress={() => run(action)}
            >
              <Icon className="size-4" /> {t(actionLabels[action])}
            </Button>
          );
        })}
      </div>
      {error && (
        <p role="alert" className="mt-2 text-sm text-danger">
          {error.message}
        </p>
      )}
      <Disclosure className="mt-3">
        <Heading>
          <Button slot="trigger" data-spatial="focusable" variant="ghost" size="sm">
            <Trans>Technical details</Trans>
          </Button>
        </Heading>
        <DisclosurePanel>
          <pre className="mt-2 whitespace-pre-wrap break-all font-mono text-xs text-fg-muted select-text">
            {failure.kind} · {failure.stage}
            {failure.occurrences > 1
              ? ` · ${plural(failure.occurrences, { one: "# time in a row", other: "# times in a row" })}`
              : ""}
            {"\n"}
            {failure.detail}
          </pre>
        </DisclosurePanel>
      </Disclosure>
      {choosingRoot && storageApi && (
        <ModalFrame
          close={() => setChoosingRoot(false)}
          className="max-h-[85vh] w-full max-w-xl overflow-auto rounded-panel border border-border-strong bg-surface-raised p-5"
        >
          <Dialog aria-label={t`Choose another install location`} className="outline-none">
            <InstallRootPicker
              api={storageApi}
              alwaysUseDefault={false}
              onCancel={() => setChoosingRoot(false)}
              onSelect={(rootId) => {
                setChoosingRoot(false);
                recover.mutate({ type: "chooseDestination", rootId });
              }}
            />
          </Dialog>
        </ModalFrame>
      )}
    </section>
  );
}
