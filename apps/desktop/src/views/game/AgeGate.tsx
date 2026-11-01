/** Age limits on play (#209): a game above the signed-in account's age
 * limit starts only after an admin's PIN. The core decides and checks every
 * launch; Play asks it first, so the PIN comes before an update or a
 * download, and a launch it still refuses asks again. An approval covers one
 * start or persists for this account and game. The core keeps the approval;
 * the PIN is never kept here. */
import { t } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { useQuery } from "@tanstack/react-query";
import { Lock } from "lucide-react";
import { useState } from "react";
import { Dialog, Modal, ModalOverlay, Radio, RadioGroup } from "react-aria-components";
import type { PinCheck } from "../../api/accounts";
import { libraryKeys, useLibraryApi } from "../../api/provider";
import type { AgeRestriction } from "../../api/types";
import {
  AgeRatingLabel,
  ratingName,
  shownRating,
  usePreferredBoard,
} from "../../components/AgeRatingLabel";
import { PinPad } from "../../components/accounts/PinPad";

/** Whether the signed-in account needs an admin's PIN for the game. Under
 * the library's keys, so switching accounts asks again. */
export function useAgeRestriction(gameId: string) {
  const api = useLibraryApi();
  return useQuery({
    queryKey: [...libraryKeys.all, "age-restriction", gameId],
    queryFn: async () => (api.ageRestriction ? api.ageRestriction(gameId) : null),
    retry: false,
  });
}

/** "An admin PIN is needed to play Doom, rated PEGI 18": the preferred
 * board's rating. */
function ageGateTitle(restriction: AgeRestriction, board?: string): string {
  const game = restriction.title;
  if (restriction.minAge === null)
    return t`An admin PIN is needed to play ${game}, which is unrated`;
  const shown = shownRating(restriction.ratings, board);
  if (!shown) return t`An admin PIN is needed to play ${game}`;
  const rating = ratingName(shown);
  return t({
    message: `An admin PIN is needed to play ${game}, rated ${rating}`,
    comment: "{rating} is an age rating, e.g. PEGI 18",
  });
}

/** Choose the approval's lifetime before sending the PIN to the core. */
export function AgeGateDialog({
  restriction,
  close,
  approved,
}: {
  restriction: AgeRestriction;
  close: () => void;
  /** An admin's PIN matched: launch now. */
  approved: () => void;
}) {
  const { t } = useLingui();
  const api = useLibraryApi();
  const [scope, setScope] = useState("once");
  const game = restriction.title;
  const account = restriction.accountName;
  const title = ageGateTitle(restriction, usePreferredBoard());
  const check = (pin: string): Promise<PinCheck> =>
    api.approvePlay
      ? api.approvePlay(restriction.gameId, restriction.accountId, pin, scope === "always")
      : Promise.reject(new Error(t`Admin approval isn't available here`));
  return (
    <ModalOverlay
      isOpen
      isDismissable
      onOpenChange={(open) => {
        if (!open) close();
      }}
      className="fixed inset-0 z-50 grid place-items-center bg-surface-base/80 p-4 backdrop-blur-sm"
    >
      <Modal className="max-h-[85vh] w-full max-w-sm overflow-y-auto rounded-panel border border-border-strong bg-surface-raised p-6 shadow-lg">
        <Dialog aria-label={title} className="flex flex-col items-center gap-4 outline-none">
          <Lock aria-hidden="true" className="size-6 text-fg-muted" />
          {restriction.ratings.length > 0 && <AgeRatingLabel ratings={restriction.ratings} />}
          <p className="text-center text-sm text-fg-muted">
            <Trans>
              Approve {game} for {account}.
            </Trans>
          </p>
          <RadioGroup
            aria-label={t`Approval duration`}
            value={scope}
            onChange={setScope}
            className="flex w-full flex-col gap-2"
          >
            {[
              { value: "once", label: t`Unlock this time` },
              { value: "always", label: t`Always allow this game for this account` },
            ].map(({ value, label }) => (
              <Radio
                key={value}
                value={value}
                data-spatial="focusable"
                className="cursor-pointer rounded-control border border-border-strong px-3 py-2 text-sm text-fg-muted outline-none data-focus-visible:focus-ring data-selected:border-accent-alt data-selected:bg-accent/25 data-selected:text-fg"
              >
                {label}
              </Radio>
            ))}
          </RadioGroup>
          <PinPad title={title} check={check} onMatched={approved} onCancel={close} />
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}

/** "Admin PIN needed" beside a game's rating, for the account signed in. */
export function AgeLock({ gameId }: { gameId: string }) {
  const restriction = useAgeRestriction(gameId).data;
  const board = usePreferredBoard();
  if (!restriction) return null;
  return <AgeLockLabel title={ageGateTitle(restriction, board)} />;
}

/** Also used by search's summaries, without a query for each game. */
export function AgeLockLabel({ title }: { title?: string }) {
  return (
    <span
      data-age-lock
      title={title}
      className="inline-flex items-center gap-1 text-xs text-fg-muted"
    >
      <Lock aria-hidden="true" className="size-3.5" />
      <Trans>Admin PIN needed</Trans>
    </span>
  );
}
