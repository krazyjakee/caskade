/** Add game → Receive a shared game… (#247): a link someone shared, pasted
 * (a pad types or pastes it on the on-screen keyboard). Caskade asks the
 * sharer what it shares first; nothing comes until Receive. The game then
 * arrives as an install in Activity. */
import { Button, Checkbox, TextField } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { useMutation } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { Download, ListPlus, Search } from "lucide-react";
import { useState } from "react";
import { Dialog, Form, Heading, Modal, ModalOverlay } from "react-aria-components";
import { type ShareInfo, useSharesApi } from "../../api/shares";
import { useActivityStorage } from "../../views/activity/ActivityProvider";
import { FailureNotice } from "../FailureNotice";
import { InstallRootPicker } from "../storage/InstallRootPicker";
import { ShareInfoDetails } from "./ShareInfoDetails";

export function ReceiveShareDialog({
  close,
  onReceived,
}: {
  close: () => void;
  /** The install job the share arrives as. */
  onReceived?: (job: string) => void;
}) {
  const { t } = useLingui();
  const api = useSharesApi();
  const [link, setLink] = useState("");
  const [help, setHelp] = useState(false);
  const inspect = useMutation({ mutationFn: (text: string) => api.inspect(text) });
  const navigate = useNavigate();
  // A collection is a list: it joins the account's collections, which open.
  const addList = useMutation({
    mutationFn: () => api.receiveCollection(link.trim()),
    onSuccess: (collection) => {
      close();
      void navigate({
        to: "/collections/$collectionId",
        params: { collectionId: collection.id },
      });
    },
  });
  // Where it goes: the storage picker, which skips itself when there's
  // nothing to choose.
  const storage = useActivityStorage();
  const [choosing, setChoosing] = useState(false);
  const receive = useMutation({
    mutationFn: async (rootId?: string) => {
      const job = await api.receive(link.trim(), rootId);
      if (help) await api.keepHelping(job.id, true);
      return job.id;
    },
    onSuccess: (job) => {
      onReceived?.(job);
      close();
    },
    onError: () => setChoosing(false),
  });
  const info: ShareInfo | undefined = inspect.data;
  const list = info?.contents.kind === "collection";
  const busy = receive.isPending || addList.isPending;
  return (
    <ModalOverlay
      isOpen
      isDismissable={!busy}
      onOpenChange={(open) => {
        if (!open) close();
      }}
      className="fixed inset-0 z-50 grid place-items-center bg-surface-base/80 p-4 backdrop-blur-sm"
    >
      <Modal className="max-h-[90vh] w-full max-w-lg overflow-auto rounded-panel border border-border-strong bg-surface-raised p-6">
        <Dialog data-cs="receive-share" className="space-y-4 outline-none">
          {choosing && storage && info ? (
            <InstallRootPicker
              api={storage}
              estimatedBytes={info.size ?? undefined}
              onCancel={() => setChoosing(false)}
              onSelect={(rootId) => receive.mutate(rootId)}
            />
          ) : (
            <>
              <Heading slot="title" className="font-display text-xl font-semibold">
                <Trans>Receive a shared game</Trans>
              </Heading>
              <Form
                className="space-y-4"
                onSubmit={(event) => {
                  event.preventDefault();
                  if (link.trim()) inspect.mutate(link.trim());
                }}
              >
                <TextField
                  label={t`Share link`}
                  value={link}
                  onChange={(value) => {
                    setLink(value);
                    inspect.reset();
                  }}
                  mono
                  description={t`The caskade://receive link someone sent you.`}
                  action={
                    <Button
                      type="submit"
                      icon={Search}
                      data-spatial="focusable"
                      isDisabled={!link.trim() || inspect.isPending}
                    >
                      <Trans>Look</Trans>
                    </Button>
                  }
                />
              </Form>
              {inspect.isPending && (
                <p role="status" className="text-sm text-fg-muted">
                  <Trans>Asking whoever shared it…</Trans>
                </p>
              )}
              {inspect.error && <FailureNotice variant="inline" error={inspect.error} />}
              {info && <ShareInfoDetails info={info} />}
              {info && !list && (
                <>
                  <Checkbox isSelected={help} onChange={setHelp} data-spatial="focusable">
                    <Trans>Help others receive it after it's here</Trans>
                  </Checkbox>
                  <p className="text-xs text-fg-subtle">
                    <Trans>
                      While it arrives, this computer helps the share's other recipients. After, it
                      keeps a second copy of the files to go on helping until the share stops.
                    </Trans>
                  </p>
                </>
              )}
              {(receive.error ?? addList.error) && (
                <FailureNotice variant="inline" error={receive.error ?? addList.error} />
              )}
              <div className="flex justify-end gap-2">
                <Button data-spatial="focusable" onPress={close} isDisabled={busy}>
                  <Trans>Cancel</Trans>
                </Button>
                {list ? (
                  <Button
                    variant="primary"
                    icon={ListPlus}
                    data-spatial="focusable"
                    isDisabled={busy}
                    onPress={() => addList.mutate()}
                  >
                    <Trans>Add to my collections</Trans>
                  </Button>
                ) : (
                  <Button
                    variant="primary"
                    icon={Download}
                    data-spatial="focusable"
                    data-focus-default={info ? true : undefined}
                    isDisabled={!info || busy}
                    onPress={() => (storage ? setChoosing(true) : receive.mutate(undefined))}
                  >
                    <Trans>Receive</Trans>
                  </Button>
                )}
              </div>
            </>
          )}
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}
