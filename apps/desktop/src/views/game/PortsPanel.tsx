import { Button, Pill } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { useQueryClient } from "@tanstack/react-query";
import { CircleCheck, Download, ExternalLink } from "lucide-react";
import { useState } from "react";
import { Dialog, Heading } from "react-aria-components";
import {
  gamePortsKey,
  type Port,
  type PortNeeds,
  systemName,
  useGamePorts,
  useInstallPort,
} from "../../api/ports";
import { ModalFrame } from "../../components/ModalFrame";
import { activityKey, useFollowJob } from "../activity/ActivityProvider";
import { JobProgressText } from "../activity/JobProgressText";

function ExternalAnchor({ href, children }: { href: string; children: React.ReactNode }) {
  return (
    <a
      href={href}
      target="_blank"
      rel="noreferrer"
      className="inline-flex items-center gap-1 text-accent-alt hover:underline"
    >
      {children}
      <ExternalLink aria-hidden="true" className="size-3" />
    </a>
  );
}

/** What a port wants from the user, and how it gets it. */
function Need({ need }: { need: PortNeeds }) {
  const { t } = useLingui();
  const how =
    need.how === "folder"
      ? need.have
        ? t`Installing copies yours in.`
        : t`Put it in the port's folder once installed.`
      : need.how === "commandLine"
        ? need.have
          ? t`Caskade gives it yours when it starts.`
          : t`It takes the file when it starts.`
        : t`It asks for the file the first time it starts.`;
  const region = need.region;
  return (
    <li>
      <span className="text-fg">{need.label}</span>
      {region && (
        <span className="text-fg-muted">
          {" "}
          <Trans comment="The region of a game file, e.g. USA">({region} version)</Trans>
        </span>
      )}
      {need.kind === "game" && (
        <span className={need.have ? "text-accent-alt" : "text-fg-muted"}>
          {" · "}
          {need.have ? t`in your library` : t`not in your library`}
        </span>
      )}
      <span className="text-fg-muted"> · {how}</span>
    </li>
  );
}

/** A port install as it runs. */
function PortProgress({ jobId }: { jobId: string }) {
  const { t } = useLingui();
  return (
    <JobProgressText
      jobId={jobId}
      starting={<Trans>Starting the install…</Trans>}
      completed={<Trans>Installed: it's in Play from your library.</Trans>}
      ended={({ status, error }) =>
        status === "cancelled" ? t`The install was cancelled.` : t`Install failed: ${error}`
      }
      needsUser={<Trans>The install needs a choice in Activity.</Trans>}
      running={(_, percent) => (percent === null ? t`Installing…` : t`Installing… ${percent}%`)}
    />
  );
}

/** Asks before installing: whose code it is, what downloads, what it needs. */
function InstallPortDialog({
  port,
  pending,
  error,
  confirm,
  close,
}: {
  port: Port;
  pending: boolean;
  error: string | null;
  confirm: () => void;
  close: () => void;
}) {
  const { t } = useLingui();
  if (port.here.state !== "installable") return null;
  const { asset, release, checked } = port.here;
  const repository = `${port.owner}/${port.name}`;
  return (
    <ModalFrame
      busy={pending}
      close={close}
      className="max-h-[90vh] w-full max-w-lg overflow-y-auto rounded-panel border border-border-strong bg-surface-raised p-6 shadow-lg"
    >
      <Dialog className="space-y-4 outline-none" aria-label={t`Install ${repository}?`}>
        <Heading slot="title" className="font-display text-xl font-semibold">
          <Trans>Install {repository}?</Trans>
        </Heading>
        <p className="text-sm text-fg-muted">
          <Trans>
            Caskade downloads {asset} ({release}) from {repository} on GitHub and adds it to this
            game as another way to play. It's a fan project, not the publisher's: installing it runs
            code from that repository.
          </Trans>
        </p>
        <p className="text-sm text-fg-muted">
          {checked
            ? t`The download is checked against the checksum in its install plan.`
            : t`Its install plan has no checksum for the download, so it can't be checked.`}
        </p>
        {port.needs.length > 0 && (
          <div className="text-sm">
            <h3 className="font-semibold">
              <Trans>It needs</Trans>
            </h3>
            <ul className="mt-1 list-disc space-y-1 pl-5">
              {port.needs.map((need) => (
                <Need key={need.label} need={need} />
              ))}
            </ul>
          </div>
        )}
        {port.notes.length > 0 && (
          <div className="text-sm">
            <h3 className="font-semibold">
              <Trans>Notes</Trans>
            </h3>
            <ul className="mt-1 list-disc space-y-1 pl-5 text-fg-muted">
              {port.notes.map((note) => (
                <li key={note}>{note}</li>
              ))}
            </ul>
          </div>
        )}
        {port.unsure.length > 0 && (
          <div className="text-sm">
            <h3 className="font-semibold">
              <Trans>Not certain in the plan</Trans>
            </h3>
            <ul className="mt-1 list-disc space-y-1 pl-5 text-fg-muted">
              {port.unsure.map((note) => (
                <li key={note}>{note}</li>
              ))}
            </ul>
          </div>
        )}
        {error && (
          <p role="alert" className="text-sm text-danger">
            {error}
          </p>
        )}
        <div className="flex justify-end gap-2">
          <Button data-spatial="focusable" isDisabled={pending} onPress={close}>
            <Trans>Cancel</Trans>
          </Button>
          <Button
            data-spatial="focusable"
            data-focus-default
            variant="primary"
            icon={Download}
            isDisabled={pending}
            onPress={confirm}
          >
            {pending ? t`Starting…` : t({ message: "Install", context: "port" })}
          </Button>
        </div>
      </Dialog>
    </ModalFrame>
  );
}

function PortRow({ gameId, port }: { gameId: string; port: Port }) {
  const { t } = useLingui();
  const install = useInstallPort(gameId);
  const follow = useFollowJob();
  const client = useQueryClient();
  const [asking, setAsking] = useState(false);
  const [jobId, setJobId] = useState<string | null>(null);
  const systems = port.runsOn.map(systemName).join(", ");
  const console = port.console;
  return (
    <li
      data-part="port"
      data-status={port.entryId ? "installed" : port.here.state}
      className="rounded-control border border-border px-4 py-3 text-sm"
    >
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0 space-y-1">
          <p className="font-semibold">
            <ExternalAnchor href={port.url}>
              {port.owner}/{port.name}
            </ExternalAnchor>
          </p>
          <p className="text-xs text-fg-muted">
            {port.gameTitle} · {console}
            {systems && (
              <>
                {" · "}
                <Trans comment="Operating systems a port has releases for, e.g. Windows, Linux">
                  runs on {systems}
                </Trans>
              </>
            )}
          </p>
          <p className="text-xs text-fg-muted">
            {port.matchedBy === "igdb"
              ? t`The same game on IGDB`
              : t`Matched by title: check it's this game`}
          </p>
        </div>
        <div className="flex shrink-0 flex-col items-end gap-1">
          {port.entryId ? (
            <Pill tone="accent" icon={CircleCheck}>
              <Trans>Installed</Trans>
            </Pill>
          ) : port.here.state === "installable" ? (
            <Button
              size="sm"
              variant="primary"
              icon={Download}
              isDisabled={install.isPending || jobId !== null}
              onPress={() => setAsking(true)}
            >
              {t({ message: "Install", context: "port" })}
            </Button>
          ) : port.here.state === "buildFromSource" ? (
            <p className="text-xs text-fg-muted">
              <Trans>Built from source on this system</Trans>
            </p>
          ) : (
            <p className="text-xs text-fg-muted">
              <Trans>No release for this system</Trans>
            </p>
          )}
        </div>
      </div>
      {port.needs.length > 0 && (
        <ul className="mt-2 space-y-0.5 text-xs">
          {port.needs.map((need) => (
            <Need key={need.label} need={need} />
          ))}
        </ul>
      )}
      {jobId && (
        <div className="mt-2 text-xs">
          <PortProgress jobId={jobId} />
        </div>
      )}
      {asking && (
        <InstallPortDialog
          port={port}
          pending={install.isPending}
          error={install.error?.message ?? null}
          close={() => setAsking(false)}
          confirm={() =>
            install.mutate(port.project, {
              onSuccess: (id) => {
                follow(id);
                setJobId(id);
                setAsking(false);
                void client.invalidateQueries({ queryKey: activityKey });
                void client.invalidateQueries({ queryKey: gamePortsKey(gameId) });
              },
            })
          }
        />
      )}
    </li>
  );
}

/** Fan ports of the game: decompilations, recompilations and source ports
 * that run it natively from the user's own copy. Nothing shows while the
 * catalogue is off, missing, or has none for the game. */
export function PortsPanel({ gameId }: { gameId: string }) {
  const ports = useGamePorts(gameId).data;
  if (!ports?.status.enabled || ports.ports.length === 0) return null;
  return (
    <section className="mt-8" aria-labelledby="ports-heading" data-part="ports">
      <h2 id="ports-heading" className="font-display text-xl font-semibold">
        <Trans>Ports</Trans>
      </h2>
      <p className="mt-1 text-sm text-fg-muted">
        <Trans>
          Fan decompilations, recompilations and source ports that run this game natively, from your
          own copy.
        </Trans>{" "}
        <ExternalAnchor href={ports.status.creditUrl}>
          <Trans>Install plans from Decomp Buddy</Trans>
        </ExternalAnchor>
      </p>
      <ul className="mt-4 space-y-2">
        {ports.ports.map((port) => (
          <PortRow key={port.project} gameId={gameId} port={port} />
        ))}
      </ul>
    </section>
  );
}
