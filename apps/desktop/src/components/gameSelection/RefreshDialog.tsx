/** Starts refreshing the chosen games' details and artwork (#176) from the
 * built-in sources (IGDB with Hasheous and store matching, RomM's ids):
 * missing details only, or the automatic ones fetched again. The user's own
 * fields, artwork choices and "not on IGDB" are always kept. */
import { Button, SegmentedControl } from "@caskade/ui";
import { plural } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { Link } from "@tanstack/react-router";
import { useState } from "react";
import { Dialog, Heading, Modal, ModalOverlay } from "react-aria-components";
import { type RefreshMode, useGamesRefresh } from "../../api/gameEdit";

export function RefreshDialog({ gameIds, close }: { gameIds: string[]; close: () => void }) {
  const { t } = useLingui();
  const refresh = useGamesRefresh();
  const [mode, setMode] = useState<RefreshMode>("missing");
  const started = refresh?.start.isSuccess ?? false;
  return (
    <ModalOverlay
      isOpen
      isDismissable
      onOpenChange={(open) => {
        if (!open) close();
      }}
      className="fixed inset-0 z-50 grid place-items-center bg-surface-base/80 p-4 backdrop-blur-sm"
    >
      <Modal className="w-full max-w-lg rounded-panel border border-border-strong bg-surface-raised p-6 shadow-lg">
        <Dialog data-cs="games-refresh-start" className="flex flex-col gap-4 outline-none">
          <Heading slot="title" className="font-display text-xl font-semibold">
            {plural(gameIds.length, {
              one: "Refresh # game's details",
              other: "Refresh # games' details",
            })}
          </Heading>
          {started ? (
            <p className="text-sm">
              <Trans>
                Refreshing in the background.{" "}
                <Link to="/activity" search={{ tab: "jobs" }} className="text-accent-alt">
                  Follow it in Activity
                </Link>
                .
              </Trans>
            </p>
          ) : (
            <>
              <SegmentedControl
                aria-label={t`What to refresh`}
                value={mode}
                onChange={setMode}
                options={[
                  { id: "missing", label: t`Missing details only` },
                  { id: "automatic", label: t`Refresh automatic details` },
                ]}
              />
              <p className="text-sm text-fg-muted">
                {mode === "missing"
                  ? t`Matches games that have no match yet and fills in what they lack from what Caskade already has.`
                  : t`Also fetches matched games again from the metadata server and replaces their automatic details and artwork.`}{" "}
                <Trans>
                  Details and artwork you set, and games you marked as not on IGDB, stay as they
                  are.
                </Trans>
              </p>
            </>
          )}
          {refresh?.start.error && (
            <p role="alert" className="text-sm text-danger">
              {refresh.start.error.message}
            </p>
          )}
          <div className="flex justify-end gap-2">
            <Button data-spatial="focusable" onPress={close}>
              {started ? <Trans>Close</Trans> : <Trans>Cancel</Trans>}
            </Button>
            {!started && refresh && (
              <Button
                data-part="start"
                data-spatial="focusable"
                variant="primary"
                isDisabled={refresh.start.isPending}
                onPress={() => refresh.start.mutate({ gameIds, mode })}
              >
                <Trans>Refresh</Trans>
              </Button>
            )}
          </div>
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}
