import { isTauri } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import appConfig from "../../src-tauri/tauri.conf.json";
import { type AppStatus, commands, events, type Snapshot } from "../api/bindings";
import { effectSubscription } from "../api/subscription";

export function useBackgroundStatus() {
  const [status, setStatus] = useState<AppStatus>({
    version: appConfig.version,
    background: { revision: 0, tasks: [] },
  });
  const [connection, setConnection] = useState<"checking" | "ready" | "error">(
    isTauri() ? "checking" : "ready",
  );

  useEffect(() => {
    if (!isTauri()) return;
    let disposed = false;
    let subscribed = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const accept = (background: Snapshot, version?: string) => {
      if (disposed) return;
      setStatus((old) => ({
        version: version ?? old.version,
        background: background.revision >= old.background.revision ? background : old.background,
      }));
      setConnection("ready");
    };
    const refresh = async () => {
      try {
        const next = await commands.backgroundStatus();
        accept(next.background, next.version);
      } catch {
        if (!disposed) setConnection("error");
      }
      // Events drive updates; a slow refresh also recovers from a missed event
      // or temporary IPC failure without continuously polling an idle app.
      if (!disposed) timer = setTimeout(() => void refresh(), subscribed ? 15_000 : 1_000);
    };
    const stop = effectSubscription(
      events.backgroundChanged.listen,
      ({ payload }) => accept(payload),
      {
        subscribed: () => {
          subscribed = true;
          void refresh();
        },
        failed: () => {
          setConnection("error");
          void refresh();
        },
      },
    );
    return () => {
      disposed = true;
      stop();
      clearTimeout(timer);
    };
  }, []);

  return { ...status, connection };
}
