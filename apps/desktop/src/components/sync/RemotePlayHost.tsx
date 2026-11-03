/** Games another device asks to start here (#234): each is played as Play
 * here would, with this device's own account, so an age limit asks for the
 * admin PIN here and console mode shows the game as for a local launch.
 * How it went goes back to the device that asked. */
import { useLingui } from "@lingui/react/macro";
import { useEffect, useRef, useState } from "react";
import { useGame } from "../../api/hooks";
import { useUnavailableStorageEntries } from "../../api/StorageProvider";
import { type PlayRequest, type SyncApi, useProvidedSyncApi } from "../../api/sync";
import { useGameLaunch } from "../../views/game/launch";
import { playableEntry } from "../gameMenu/GameMenu";

export function RemotePlayHost() {
  const api = useProvidedSyncApi();
  const [requests, setRequests] = useState<PlayRequest[]>([]);
  useEffect(() => api?.onPlayRequest((request) => setRequests((all) => [...all, request])), [api]);
  if (!api) return null;
  return requests.map((request) => (
    <Start
      key={request.requestId}
      request={request}
      api={api}
      done={() => setRequests((all) => all.filter((r) => r.requestId !== request.requestId))}
    />
  ));
}

function Start({ request, api, done }: { request: PlayRequest; api: SyncApi; done: () => void }) {
  const { t } = useLingui();
  const flow = useGameLaunch(request.gameId);
  const game = useGame(request.gameId).data;
  const unavailable = useUnavailableStorageEntries();
  const started = useRef(false);
  const answered = useRef(false);
  const answer = (ok: boolean, reason?: string) => {
    if (answered.current) return;
    answered.current = true;
    void api.finishPlay(request.requestId, ok, reason);
    done();
  };
  useEffect(() => {
    if (started.current || !game) return;
    started.current = true;
    const entry = playableEntry(game.entries, unavailable, game.defaultEntryId);
    if (entry) flow.play(entry.id);
    else answer(false, t`It has nothing installed here that can launch.`);
  });
  useEffect(() => {
    if (flow.launch.isSuccess) answer(true);
    else if (flow.launch.isError && !flow.asking) answer(false, flow.launch.error.message);
  });
  return <>{flow.dialogs}</>;
}
