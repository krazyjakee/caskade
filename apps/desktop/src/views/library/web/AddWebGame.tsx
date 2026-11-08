import { Button, Checkbox, TextField } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Plus } from "lucide-react";
import { useState } from "react";
import { Dialog, Form, Heading, Modal, ModalOverlay } from "react-aria-components";
import { libraryKeys, useLibraryApi } from "../../../api/provider";
import { InstallRootPicker } from "../../../components/storage/InstallRootPicker";
import { useActivityStorage } from "../../activity/ActivityProvider";

const overlay =
  "fixed inset-0 z-50 grid place-items-center bg-surface-base/80 p-4 backdrop-blur-sm";

/** The Web tab's Add web game: a page address, and the
 * game is in the library. With Play offline, Caskade then asks where to
 * keep it and opens the guided capture (§6). */
export function AddWebGame() {
  const { t } = useLingui();
  const api = useLibraryApi();
  const storage = useActivityStorage();
  const client = useQueryClient();
  const [open, setOpen] = useState(false);
  const [url, setUrl] = useState("");
  const [offline, setOffline] = useState(false);
  // The game added, waiting for where to keep its offline copy.
  const [placing, setPlacing] = useState<string | null>(null);
  const canOffline = Boolean(api.saveWebGameOffline && storage);
  const done = () => {
    void client.invalidateQueries({ queryKey: libraryKeys.all });
    setOpen(false);
    setUrl("");
    setOffline(false);
  };
  const add = useMutation({
    mutationFn: (page: string) => {
      if (!api.addWebGame) throw new Error(t`Web games aren't available here`);
      return api.addWebGame(page);
    },
    onSuccess: (gameId) => {
      if (offline && canOffline) {
        setOpen(false);
        setPlacing(gameId);
      } else done();
    },
  });
  const save = useMutation({
    mutationFn: ({ gameId, rootId }: { gameId: string; rootId: string }) => {
      if (!api.saveWebGameOffline) throw new Error(t`Offline web games aren't available here`);
      return api.saveWebGameOffline(gameId, rootId);
    },
    onSettled: done,
  });
  if (!api.addWebGame) return null;
  const error = add.error ?? save.error;
  return (
    <div>
      <Button
        icon={Plus}
        variant="primary"
        data-spatial="focusable"
        onPress={() => {
          add.reset();
          save.reset();
          setOpen(true);
        }}
      >
        <Trans>Add web game</Trans>
      </Button>
      {error && !open && (
        <p role="alert" className="mt-2 max-w-sm text-sm text-danger">
          {error.message}
        </p>
      )}
      <ModalOverlay
        isOpen={open}
        onOpenChange={setOpen}
        isDismissable={!add.isPending}
        className={overlay}
      >
        <Modal className="w-full max-w-lg rounded-panel border border-border-strong bg-surface-raised p-6">
          <Dialog data-cs="add-web-game" className="outline-none">
            <Heading slot="title" className="font-display text-xl font-semibold">
              <Trans>Add web game</Trans>
            </Heading>
            <Form
              className="mt-4 space-y-4"
              onSubmit={(event) => {
                event.preventDefault();
                add.mutate(url.trim());
              }}
            >
              <TextField
                label={t`Game page`}
                autoFocus
                type="url"
                placeholder="https://"
                value={url}
                onChange={setUrl}
                isRequired
                isDisabled={add.isPending}
                description={t`The address of the page the game plays on.`}
              />
              {canOffline && (
                <div className="space-y-1">
                  <Checkbox isSelected={offline} onChange={setOffline} isDisabled={add.isPending}>
                    <Trans>Play offline</Trans>
                  </Checkbox>
                  <p className="pl-7.5 text-xs text-fg-muted">
                    <Trans>
                      Caskade keeps a copy of the game. You'll play it for a minute so it can see
                      what the game needs.
                    </Trans>
                  </p>
                </div>
              )}
              {add.error && (
                <p role="alert" className="text-sm text-danger">
                  {add.error.message}
                </p>
              )}
              <div className="flex justify-end gap-2">
                <Button onPress={() => setOpen(false)} isDisabled={add.isPending}>
                  <Trans>Cancel</Trans>
                </Button>
                <Button type="submit" variant="primary" isDisabled={!url.trim() || add.isPending}>
                  {add.isPending ? t`Adding…` : t`Add`}
                </Button>
              </div>
            </Form>
          </Dialog>
        </Modal>
      </ModalOverlay>
      {placing && storage && (
        <ModalOverlay
          isOpen
          isDismissable
          onOpenChange={(keep) => {
            if (!keep) {
              setPlacing(null);
              done();
            }
          }}
          className={overlay}
        >
          <Modal className="max-h-[85vh] w-full max-w-xl overflow-auto rounded-panel border border-border-strong bg-surface-raised p-5">
            <Dialog aria-label={t`Where to keep the game`} className="outline-none">
              <InstallRootPicker
                api={storage}
                onCancel={() => {
                  setPlacing(null);
                  done();
                }}
                onSelect={(rootId) => {
                  const gameId = placing;
                  setPlacing(null);
                  save.mutate({ gameId, rootId });
                }}
              />
            </Dialog>
          </Modal>
        </ModalOverlay>
      )}
    </div>
  );
}
