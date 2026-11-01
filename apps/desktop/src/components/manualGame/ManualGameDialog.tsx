/** Adds a game by hand (#174): a game already on disk or behind a URI,
 * started where it is (nothing is copied, installed or run), or a
 * title-only record of a game Caskade can't start (a cartridge, a disc on a
 * shelf). A game that may already be in the library offers to add the
 * target to it, or to keep the new one apart. */
import { Button, TextField } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { useNavigate } from "@tanstack/react-router";
import { FileSearch } from "lucide-react";
import { useState } from "react";
import { Dialog, Form, Heading, Modal, ModalOverlay } from "react-aria-components";
import {
  type DuplicateGame,
  type ManualTarget,
  useAddManualGame,
  useManualGamesApi,
} from "../../api/manualGames";
import { errorMessage } from "../../api/reply";
import { useDropZone } from "../../drop/DropProvider";
import { usePathPicker } from "../pathBrowser/PathPicker";

export type ManualMode = "target" | "title";

export function ManualGameDialog({ mode, close }: { mode: ManualMode; close: () => void }) {
  const { t } = useLingui();
  const api = useManualGamesApi();
  const add = useAddManualGame();
  const navigate = useNavigate();
  const picker = usePathPicker();
  const [input, setInput] = useState("");
  const [target, setTarget] = useState<ManualTarget | null>(null);
  const [title, setTitle] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);
  const [duplicates, setDuplicates] = useState<DuplicateGame[] | null>(null);

  const resolve = (value: string) => {
    if (!api || !value.trim()) return;
    setInput(value);
    setError(null);
    setChecking(true);
    api
      .target(value)
      .then((resolved) => {
        setTarget(resolved);
        setTitle((current) => current || resolved.title);
        setDuplicates(null);
      })
      .catch((failure: unknown) => {
        setTarget(null);
        setError(errorMessage(failure));
      })
      .finally(() => setChecking(false));
  };
  const zone = useDropZone<HTMLElement>(
    mode === "target" ? (paths) => resolve(paths[0] ?? "") : null,
  );
  const choose = () => {
    picker.pick({ kind: "file", title: t`The game's program or shortcut` }).then((path) => {
      if (path) resolve(path);
    });
  };
  const finish = (choice: { linkTo?: string; keepApart?: boolean }) =>
    add.mutate(
      {
        title: title.trim(),
        target: mode === "target" ? target : null,
        linkTo: choice.linkTo ?? null,
        keepApart: choice.keepApart ?? false,
      },
      {
        onSuccess: (id) => {
          close();
          void navigate({ to: "/games/$gameId", params: { gameId: id } });
        },
      },
    );
  const submit = () => {
    if (!api || !title.trim() || (mode === "target" && !target)) return;
    if (duplicates) return finish({ keepApart: duplicates.length > 0 });
    setChecking(true);
    api
      .duplicates(title.trim(), mode === "target" ? target : null)
      .then((found) => {
        if (found.length === 0) finish({});
        else setDuplicates(found);
      })
      .catch((failure: unknown) => setError(errorMessage(failure)))
      .finally(() => setChecking(false));
  };
  const busy = checking || add.isPending;
  const failure = error ?? add.error?.message ?? null;
  const ready = Boolean(title.trim()) && (mode === "title" || target !== null);
  return (
    <ModalOverlay
      isOpen
      isDismissable={!busy}
      onOpenChange={(open) => {
        if (!open && !busy) close();
      }}
      className="fixed inset-0 z-50 grid place-items-center bg-surface-base/80 p-4 backdrop-blur-sm console:p-0"
    >
      <Modal className="flex max-h-[88vh] w-full max-w-xl flex-col rounded-panel border border-border-strong bg-surface-raised shadow-lg console:h-full console:max-h-none console:max-w-none console:rounded-none">
        <Dialog
          data-cs="manual-game"
          data-mode={mode}
          className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto p-6 outline-none"
        >
          <Heading slot="title" className="font-display text-xl font-semibold">
            {mode === "target" ? (
              <Trans>Add a game on this computer</Trans>
            ) : (
              <Trans>Add a game by title</Trans>
            )}
          </Heading>
          <p className="text-sm text-fg-muted">
            {mode === "target" ? (
              <Trans>
                Caskade starts it where it is: nothing is copied, moved or installed. To install a
                game instead, use Install from file or drop it on the library.
              </Trans>
            ) : (
              <Trans>
                A record for a game Caskade can't start, like a cartridge or a disc: it keeps your
                notes, artwork and collections. Add something to start later in Edit details.
              </Trans>
            )}
          </p>
          <Form
            className="flex flex-col gap-4"
            onSubmit={(event) => {
              event.preventDefault();
              submit();
            }}
          >
            {mode === "target" && (
              <section
                ref={zone.ref}
                data-part="target"
                data-drop-active={zone.active || undefined}
                aria-label={t`What it starts`}
                className={`flex flex-col gap-3 rounded-panel border border-dashed p-4 ${zone.active ? "border-accent-alt" : "border-border-strong"}`}
              >
                <TextField
                  label={t`Program, shortcut or URI`}
                  description={t`A path, a .desktop, .lnk or .url shortcut, or a URI like steam://rungameid/620. Or drop a file here.`}
                  mono
                  value={input}
                  onChange={(value) => {
                    setInput(value);
                    setTarget(null);
                    setDuplicates(null);
                  }}
                  onBlur={() => {
                    if (input.trim() && !target) resolve(input);
                  }}
                  action={
                    <Button
                      data-spatial="focusable"
                      icon={FileSearch}
                      onPress={choose}
                      isDisabled={busy}
                    >
                      <Trans>Browse…</Trans>
                    </Button>
                  }
                />
                {target && <TargetSummary target={target} />}
              </section>
            )}
            <TextField
              label={t`Title`}
              value={title}
              onChange={(value) => {
                setTitle(value);
                setDuplicates(null);
              }}
              isRequired
            />
            {duplicates && duplicates.length > 0 && (
              <section
                data-part="duplicates"
                aria-label={t`Maybe already in your library`}
                className="flex flex-col gap-2 rounded-panel border border-warning/50 p-4"
              >
                <h3 className="text-sm font-semibold">
                  <Trans>This may already be in your library</Trans>
                </h3>
                <ul className="flex flex-col gap-2">
                  {duplicates.map((duplicate) => (
                    <li
                      key={duplicate.gameId}
                      data-part="duplicate"
                      className="flex flex-wrap items-center justify-between gap-2"
                    >
                      <span className="text-sm">
                        {duplicate.title}{" "}
                        <span className="text-xs text-fg-muted">
                          {duplicate.reason === "sameTarget"
                            ? t`starts the same program`
                            : t`has the same title`}
                        </span>
                      </span>
                      {mode === "target" && (
                        <Button
                          data-part="link"
                          data-spatial="focusable"
                          size="sm"
                          isDisabled={busy}
                          onPress={() => finish({ linkTo: duplicate.gameId })}
                        >
                          <Trans>Add to this game</Trans>
                        </Button>
                      )}
                    </li>
                  ))}
                </ul>
                <p className="text-xs text-fg-muted">
                  <Trans>Or keep it as a game of its own: Caskade won't merge them later.</Trans>
                </p>
              </section>
            )}
            {failure && (
              <p role="alert" data-part="error" className="text-sm text-danger">
                {failure}
              </p>
            )}
            <div className="flex justify-end gap-2">
              <Button data-spatial="focusable" onPress={close} isDisabled={busy}>
                <Trans>Cancel</Trans>
              </Button>
              <Button
                data-part="add"
                data-spatial="focusable"
                type="submit"
                variant="primary"
                isDisabled={!ready || busy}
              >
                {duplicates && duplicates.length > 0 ? (
                  <Trans>Keep separate</Trans>
                ) : busy ? (
                  <Trans>Adding…</Trans>
                ) : (
                  <Trans>Add game</Trans>
                )}
              </Button>
            </div>
          </Form>
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}

/** What the target starts, as the user should check it. */
function TargetSummary({ target }: { target: ManualTarget }) {
  const { t } = useLingui();
  const launch = target.launch;
  return (
    <dl data-part="summary" className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-xs">
      {launch.type === "executable" ? (
        <>
          <dt className="text-fg-muted">{t`Program`}</dt>
          <dd className="font-mono break-all">{launch.path}</dd>
          {launch.args.length > 0 && (
            <>
              <dt className="text-fg-muted">{t`Arguments`}</dt>
              <dd className="font-mono break-all">{launch.args.join(" ")}</dd>
            </>
          )}
          {launch.cwd && (
            <>
              <dt className="text-fg-muted">{t`Starts in`}</dt>
              <dd className="font-mono break-all">{launch.cwd}</dd>
            </>
          )}
          {target.windows && (
            <>
              <dt className="text-fg-muted">{t`Runs with`}</dt>
              <dd>{t`a Windows compatibility tool`}</dd>
            </>
          )}
        </>
      ) : launch.type === "uri" ? (
        <>
          <dt className="text-fg-muted">{t`Opens`}</dt>
          <dd className="font-mono break-all">{launch.uri}</dd>
        </>
      ) : null}
    </dl>
  );
}
