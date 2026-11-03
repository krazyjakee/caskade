import { Button, Menu, MenuItem, MenuPopover, TextField } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Plus } from "lucide-react";
import { useState } from "react";
import { Dialog, Form, Heading, MenuTrigger, Modal, ModalOverlay } from "react-aria-components";
import { type InstallInput, type JobSnapshot, mergeJobs } from "../../api/install";
import { useManualGamesApi } from "../../api/manualGames";
import { ManualGameDialog, type ManualMode } from "../../components/manualGame/ManualGameDialog";
import { usePathPicker } from "../../components/pathBrowser/PathPicker";
import { ReceiveShareDialog } from "../../components/shares/ReceiveShareDialog";
import { InstallRootPicker } from "../../components/storage/InstallRootPicker";
import { activityKey, useActivityStorage, useFollowJob, useInstallApi } from "./ActivityProvider";

export function AddGame() {
  const { t } = useLingui();
  const api = useInstallApi();
  const storageApi = useActivityStorage();
  const client = useQueryClient();
  const picker = usePathPicker();
  const follow = useFollowJob();
  const [urlOpen, setUrlOpen] = useState(false);
  const [url, setUrl] = useState("");
  const [pending, setPending] = useState<InstallInput[] | null>(null);
  // Adding a game by hand (#174), in place or as a title.
  const manual = useManualGamesApi();
  const [adding, setAdding] = useState<ManualMode | null>(null);
  // A game someone shared through a link (#247).
  const [receiving, setReceiving] = useState(false);
  const install = useMutation({
    mutationFn: async ({ inputs, rootId }: { inputs: InstallInput[]; rootId?: string }) => {
      const jobs: JobSnapshot[] = [];
      for (const input of inputs) {
        const job = await api.install(input, rootId);
        // Retain each successful enqueue even if a later selected file fails.
        client.setQueryData<JobSnapshot[]>(activityKey, (current) => mergeJobs(current, [job]));
        // A choice it needs (an asset, an installer) opens where the user is.
        follow(job.id);
        jobs.push(job);
      }
      return jobs;
    },
    onSuccess: () => {
      setUrlOpen(false);
      setUrl("");
    },
  });
  const choose = useMutation({
    mutationFn: async (
      input: { type: "url"; value: string } | { type: "file" | "folder" },
    ): Promise<InstallInput[]> => {
      const values =
        input.type === "url"
          ? [input.value]
          : // Console mode: one path from the app's own browser.
            picker.inApp
            ? [
                await picker.pick({
                  kind: input.type,
                  title: input.type === "file" ? t`Install from file` : t`Install from folder`,
                }),
              ].filter((path) => path !== null)
            : await api.choosePaths(input.type);
      return values.map((value) => ({ type: input.type === "url" ? "url" : "path", value }));
    },
    onSuccess: (inputs) => {
      if (inputs.length === 0) return;
      if (storageApi) {
        setUrlOpen(false);
        setPending(inputs);
      } else install.mutate({ inputs });
    },
  });
  const busy = install.isPending || choose.isPending;
  const error = install.error ?? choose.error;
  return (
    <div data-cs="add-game">
      <MenuTrigger>
        <Button icon={Plus} variant="primary" data-spatial="focusable" isDisabled={busy}>
          <Trans>Add game</Trans>
        </Button>
        <MenuPopover>
          <Menu
            aria-label={t`Install game from`}
            onAction={(id) => {
              install.reset();
              choose.reset();
              if (id === "url") setUrlOpen(true);
              else if (id === "manual-target") setAdding("target");
              else if (id === "manual-title") setAdding("title");
              else if (id === "share") setReceiving(true);
              else if (id === "file" || id === "folder") choose.mutate({ type: id });
            }}
          >
            {[
              {
                id: "file",
                title: t`Install from file…`,
                description: t`An installer, archive, disc image or .torrent file`,
              },
              { id: "folder", title: t`Install from folder…` },
              {
                id: "url",
                title: t`Install from link…`,
                description: t`A download link or a magnet link`,
              },
              {
                id: "share",
                title: t`Receive a shared game…`,
                description: t`A link someone sent you from their Caskade`,
              },
              ...(manual
                ? [
                    { id: "manual-target", title: t`Add a game on this computer…` },
                    { id: "manual-title", title: t`Add a game by title…` },
                  ]
                : []),
            ].map((item: { id: string; title: string; description?: string }) => (
              <MenuItem
                key={item.id}
                id={item.id}
                description={item.description}
                data-spatial="focusable"
              >
                {item.title}
              </MenuItem>
            ))}
          </Menu>
        </MenuPopover>
      </MenuTrigger>
      {adding && <ManualGameDialog mode={adding} close={() => setAdding(null)} />}
      {receiving && <ReceiveShareDialog close={() => setReceiving(false)} onReceived={follow} />}
      {error && !urlOpen && (
        <p role="alert" className="mt-2 max-w-sm text-sm text-danger">
          {error.message}
        </p>
      )}
      <ModalOverlay
        isOpen={urlOpen}
        onOpenChange={setUrlOpen}
        isDismissable={!busy}
        className="fixed inset-0 z-50 grid place-items-center bg-surface-base/80 p-4 backdrop-blur-sm"
      >
        <Modal className="w-full max-w-lg rounded-panel border border-border-strong bg-surface-raised p-6">
          <Dialog data-cs="install-url" className="outline-none">
            <Heading slot="title" className="font-display text-xl font-semibold">
              <Trans>Install from link</Trans>
            </Heading>
            <Form
              className="mt-4 space-y-4"
              onSubmit={(event) => {
                event.preventDefault();
                choose.mutate({ type: "url", value: url.trim() });
              }}
            >
              <TextField
                label={t`Download or magnet link`}
                autoFocus
                type="url"
                value={url}
                onChange={setUrl}
                isRequired
                isDisabled={busy}
                description={t`A download link or a magnet link. The installer resolves it before downloading, and you may be asked to choose what to download.`}
              />
              {error && (
                <p role="alert" className="text-sm text-danger">
                  {error.message}
                </p>
              )}
              <div className="flex justify-end gap-2">
                <Button onPress={() => setUrlOpen(false)} isDisabled={busy}>
                  <Trans>Cancel</Trans>
                </Button>
                <Button type="submit" variant="primary" isDisabled={!url.trim() || busy}>
                  {busy ? t`Adding…` : t`Add installation`}
                </Button>
              </div>
            </Form>
          </Dialog>
        </Modal>
      </ModalOverlay>
      {pending && storageApi && (
        <ModalOverlay
          isOpen
          isDismissable
          onOpenChange={(open) => {
            if (!open) setPending(null);
          }}
          className="fixed inset-0 z-50 grid place-items-center bg-surface-base/80 p-4 backdrop-blur-sm"
        >
          <Modal className="max-h-[85vh] w-full max-w-xl overflow-auto rounded-panel border border-border-strong bg-surface-raised p-5">
            <Dialog aria-label={t`Choose installation storage`} className="outline-none">
              <InstallRootPicker
                api={storageApi}
                onCancel={() => setPending(null)}
                onSelect={(rootId) => {
                  const inputs = pending;
                  setPending(null);
                  install.mutate({ inputs, rootId });
                }}
              />
            </Dialog>
          </Modal>
        </ModalOverlay>
      )}
    </div>
  );
}
