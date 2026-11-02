import { Button, TextField } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { Check, X } from "lucide-react";
import { useState } from "react";
import { type PairRequest, useStreamingAction } from "../../api/streaming";
import { FailureNotice } from "../FailureNotice";

/** A device waiting to pair with this one (#265): the code it shows, the
 * name to know it by, then Pair or Decline. In Settings → Streaming and
 * in the prompt that appears when a request arrives. */
export function PairRequestForm({
  request,
  done,
  autoFocus = false,
}: {
  request: PairRequest;
  /** Paired or declined. */
  done?: () => void;
  autoFocus?: boolean;
}) {
  const { t } = useLingui();
  const [code, setCode] = useState("");
  const [name, setName] = useState(request.name);
  const accept = useStreamingAction((api, _: undefined) =>
    api.acceptPairing(request.id, code.trim(), name.trim() || request.name),
  );
  const decline = useStreamingAction((api, _: undefined) => api.declinePairing(request.id));
  const device = request.name;
  const ready = /^\d{4}$/.test(code.trim());
  const busy = accept.isPending || decline.isPending;
  return (
    <form
      data-cs="pair-request"
      className="flex flex-col gap-3"
      onSubmit={(event) => {
        event.preventDefault();
        if (ready && !busy) accept.mutate(undefined, { onSuccess: done });
      }}
    >
      <p data-part="ask" className="text-sm">
        <Trans>{device} wants to stream games from this device.</Trans>
      </p>
      <div className="flex flex-wrap items-end gap-3">
        <TextField
          label={t`Code shown on ${device}`}
          value={code}
          onChange={(value) => setCode(value.replace(/\D/g, "").slice(0, 4))}
          inputMode="numeric"
          autoComplete="off"
          autoFocus={autoFocus}
          mono
          className="w-40"
        />
        <TextField label={t`Call it`} value={name} onChange={setName} className="min-w-48 flex-1" />
      </div>
      <div className="flex flex-wrap gap-2">
        <Button type="submit" variant="primary" icon={Check} isDisabled={!ready || busy}>
          {accept.isPending ? t`Pairing…` : t`Pair`}
        </Button>
        <Button
          icon={X}
          isDisabled={busy}
          onPress={() => decline.mutate(undefined, { onSuccess: done })}
        >
          <Trans>Decline</Trans>
        </Button>
      </div>
      {(accept.error ?? decline.error) && (
        <FailureNotice variant="inline" error={accept.error ?? decline.error} />
      )}
    </form>
  );
}
