import { t as tNow } from "@lingui/core/macro";
import { useLingui } from "@lingui/react/macro";
import { useCallback, useEffect, useRef, useState } from "react";
import { Dialog } from "react-aria-components";
import type { InstallRequestApi } from "../../api/installRequests";
import { effectSubscription } from "../../api/subscription";
import { ModalFrame } from "../../components/ModalFrame";
import { InstallRootPicker } from "../../components/storage/InstallRootPicker";
import { useDropFallback } from "../../drop/DropProvider";
import { useActivityStorage, useFollowJob } from "./ActivityProvider";
import { InstallConfirmation, type InstallRequest } from "./InstallConfirmation";

export type { InstallRequestApi } from "../../api/installRequests";

/** A paste into a text field belongs to that field. Browser File.name is
 * never treated as a native path; native file drops come through the drop
 * zones (`drop/`), and those outside every zone end up here. */
function editableTarget(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLElement &&
    Boolean(
      target.closest(
        "input,textarea,select,[contenteditable]:not([contenteditable=false]),[role=textbox],[role=combobox]",
      ),
    )
  );
}
export function installationText(value: string): string | null {
  const text = value.trim();
  if (!text || text.length > 32768 || /[\r\n\0]/.test(text)) return null;
  if (/^https?:\/\//i.test(text)) {
    try {
      const url = new URL(text);
      return url.hostname && !url.username && !url.password ? text : null;
    } catch {
      return null;
    }
  }
  // A magnet link (#159) with a BitTorrent info hash.
  if (/^magnet:\?/i.test(text)) return /[?&]xt=urn:btih:[a-z0-9]+/i.test(text) ? text : null;
  return /^(\/|[a-z]:[\\/]|\\\\[^\\]+\\)/i.test(text) ? text : null;
}
export function InstallIngress({ api }: { api: InstallRequestApi }) {
  const { t } = useLingui();
  const storageApi = useActivityStorage();
  const follow = useFollowJob();
  const [queue, setQueue] = useState<InstallRequest[]>([]);
  const [rootRequest, setRootRequest] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [rootError, setRootError] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const settled = useRef(new Set<string>());
  const receive = useCallback((request: InstallRequest) => {
    if (settled.current.has(request.requestId)) return;
    setQueue((current) =>
      current.some((item) => item.requestId === request.requestId)
        ? current
        : [...current, request],
    );
  }, []);
  useDropFallback((paths) => {
    for (const path of paths)
      api
        .request(path)
        .then(receive)
        .catch((error: unknown) =>
          setError(
            error instanceof Error ? error.message : tNow`Could not review the dropped file.`,
          ),
        );
  });
  useEffect(() => {
    let disposed = false;
    const receiveLive = (request: InstallRequest) => {
      if (!disposed) receive(request);
    };
    const failed = (error: unknown) => {
      if (!disposed)
        setError(
          error instanceof Error ? error.message : tNow`Could not receive installation requests.`,
        );
    };
    // Subscribe first, then recover requests received before the webview mounted.
    const stop = effectSubscription(api.subscribe, receiveLive, {
      subscribed: () =>
        void api
          .pending()
          .then((requests) => {
            for (const request of requests) receiveLive(request);
          })
          .catch(failed),
      failed,
    });
    const requestText = (event: ClipboardEvent | DragEvent, value: string) => {
      if (editableTarget(event.target) || editableTarget(document.activeElement)) return;
      const input = installationText(value);
      if (!input) return;
      event.preventDefault();
      void api
        .request(input)
        .then(receiveLive)
        .catch((error: unknown) => {
          if (!disposed)
            setError(
              error instanceof Error ? error.message : tNow`Could not review installation input.`,
            );
        });
    };
    const paste = (event: ClipboardEvent) =>
      requestText(event, event.clipboardData?.getData("text/plain") ?? "");
    const drop = (event: DragEvent) => {
      if (event.dataTransfer?.files.length) return;
      const uri = event.dataTransfer
        ?.getData("text/uri-list")
        .split(/\r?\n/)
        .filter((line) => line && !line.startsWith("#"))
        .join("\n");
      requestText(event, uri || event.dataTransfer?.getData("text/plain") || "");
    };
    const drag = (event: DragEvent) => {
      if (!editableTarget(event.target) && event.dataTransfer?.types.includes("text/uri-list"))
        event.preventDefault();
    };
    window.addEventListener("paste", paste);
    window.addEventListener("drop", drop);
    window.addEventListener("dragover", drag);
    return () => {
      disposed = true;
      stop();
      window.removeEventListener("paste", paste);
      window.removeEventListener("drop", drop);
      window.removeEventListener("dragover", drag);
    };
  }, [api, receive]);
  const finish = async (id: string, accept: boolean, rootId?: string) => {
    // A shared collection is a list: nothing installs, so no storage (#247).
    const list =
      queue.find((request) => request.requestId === id)?.share?.contents.kind === "collection";
    if (accept && !list && storageApi && rootId === undefined) {
      setRootError(null);
      setRootRequest(id);
      return;
    }
    if (accept && list && api.confirmCollection) await api.confirmCollection(id);
    else if (accept)
      // A choice it needs (an asset, an installer) opens where the user is.
      follow(rootId === undefined ? await api.confirm(id) : await api.confirm(id, rootId));
    else await api.dismiss(id);
    settled.current.add(id);
    // Backend queues are bounded; keep only a bounded local replay guard too.
    if (settled.current.size > 1024) {
      const oldest = settled.current.values().next().value;
      if (oldest) settled.current.delete(oldest);
    }
    setQueue((current) => current.filter((request) => request.requestId !== id));
    setRootRequest(null);
  };
  const request = queue[0];
  return (
    <>
      {error && (
        <p
          role="alert"
          data-cs="install-input-error"
          className="m-3 rounded-control border border-danger p-3 text-sm text-danger"
        >
          {error}
        </p>
      )}
      {request && !rootRequest && (
        <InstallConfirmation
          key={request.requestId}
          request={request}
          confirm={(id) => finish(id, true)}
          dismiss={(id) => finish(id, false)}
        />
      )}
      {rootRequest && storageApi && (
        <ModalFrame
          busy={confirming}
          close={() => setRootRequest(null)}
          className="max-h-[85vh] w-full max-w-xl overflow-auto rounded-panel border border-border-strong bg-surface-raised p-5"
        >
          <Dialog aria-label={t`Choose installation storage`} className="outline-none">
            {rootError && (
              <p role="alert" className="mb-3 text-sm text-danger">
                {rootError}
              </p>
            )}
            <div inert={confirming} aria-busy={confirming}>
              <InstallRootPicker
                api={storageApi}
                onCancel={() => setRootRequest(null)}
                onSelect={(rootId) => {
                  setConfirming(true);
                  void finish(rootRequest, true, rootId)
                    .catch((error: unknown) =>
                      setRootError(
                        error instanceof Error
                          ? error.message
                          : tNow`Unable to confirm installation.`,
                      ),
                    )
                    .finally(() => setConfirming(false));
                }}
              />
            </div>
          </Dialog>
        </ModalFrame>
      )}
    </>
  );
}
