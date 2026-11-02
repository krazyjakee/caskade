/** A game's editor (#172): its details in one draft saved as a patch, and
 * tabs other features fill (launch actions, scripts), each saving on its
 * own. Closing with unsaved work anywhere asks first. Desktop mode shows it
 * as a dialog, console mode over the whole window. */
import { Button } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { useRef, useState } from "react";
import {
  Dialog,
  Heading,
  type Key,
  Modal,
  ModalOverlay,
  Tab,
  TabList,
  TabPanel,
  Tabs,
} from "react-aria-components";
import {
  type EditableGame,
  type FieldError,
  type GameField,
  useGameEditState,
  useSaveGameEdit,
} from "../../api/gameEdit";
import { useGame } from "../../api/hooks";
import { useInputAction } from "../../input";
import { ActionsTab } from "./ActionsTab";
import { ArtworkTab } from "./ArtworkTab";
import { DetailsTab } from "./DetailsTab";
import { type Draft, isDirty, prune, toEdit } from "./draft";
import { SavesTab } from "./SavesTab";
import { ScriptsTab } from "./ScriptsTab";

type TabId = "details" | "artwork" | "actions" | "scripts" | "saves";
const TABS: TabId[] = ["details", "artwork", "actions", "scripts", "saves"];

export function GameEditor({
  gameId,
  title,
  close,
}: {
  gameId: string;
  title: string;
  close: () => void;
}) {
  const { t } = useLingui();
  const loaded = useGameEditState(gameId);
  const details = useGame(gameId).data;
  const save = useSaveGameEdit(gameId);
  const dialog = useRef<HTMLElement>(null);
  const [tab, setTab] = useState<TabId>("details");
  const [draft, setDraft] = useState<Draft>({});
  const [errors, setErrors] = useState<FieldError[]>([]);
  // The game as the editor opened it: a field saved differently since is
  // a conflict, not overwritten.
  const [base, setBase] = useState<EditableGame | null>(null);
  const state = loaded.data ?? null;
  if (state && base === null) setBase(state.game);
  const [dirtyTabs, setDirtyTabs] = useState<Partial<Record<TabId, boolean>>>({});
  const [confirming, setConfirming] = useState(false);
  const detailsDirty = isDirty(draft);
  const otherDirty = Object.values(dirtyTabs).some(Boolean);
  const requestClose = () => {
    if (detailsDirty || otherDirty) setConfirming(true);
    else close();
  };
  const onDraft = (next: Draft) => {
    setDraft(next);
    // A field changed again is checked again on the next save.
    setErrors((current) => current.filter((error) => error.field in next));
  };
  const rebase = (field: GameField) => {
    if (!state) return;
    setBase((current) => current && { ...current, [field]: state.game[field] });
    setErrors((current) => current.filter((error) => error.field !== field));
  };
  const submit = () => {
    if (!state || !detailsDirty || save.isPending) return;
    save.mutate(
      { edit: toEdit(draft), base },
      {
        onSuccess: (outcome) => {
          if (outcome.status === "rejected") {
            setErrors(outcome.errors);
            return;
          }
          setErrors([]);
          setBase(outcome.state.game);
          setDraft(prune(outcome.state, draft));
          if (!otherDirty) close();
        },
      },
    );
  };
  const step = (by: number) => {
    const next = TABS[(TABS.indexOf(tab) + by + TABS.length) % TABS.length];
    if (next) setTab(next);
  };
  useInputAction("tab.prev", () => step(-1), { label: t`Previous tab`, owner: dialog });
  useInputAction("tab.next", () => step(1), { label: t`Next tab`, owner: dialog });
  useInputAction("menu", submit, {
    label: t`Save`,
    owner: dialog,
    enabled: tab === "details" && detailsDirty,
  });
  const tabDirty = (id: TabId) => (dirty: boolean) =>
    setDirtyTabs((current) => (current[id] === dirty ? current : { ...current, [id]: dirty }));
  const failure = save.error?.message ?? loaded.error?.message;
  const labels: Record<TabId, string> = {
    details: t`Details`,
    artwork: t`Artwork`,
    actions: t`Actions`,
    scripts: t`Scripts`,
    saves: t`Saves`,
  };
  return (
    <ModalOverlay
      isOpen
      isDismissable={!confirming}
      isKeyboardDismissDisabled={confirming}
      onOpenChange={(open) => {
        if (!open) requestClose();
      }}
      className="fixed inset-0 z-50 grid place-items-center bg-surface-base/80 p-4 backdrop-blur-sm console:p-0"
    >
      <Modal className="flex h-[88vh] w-full max-w-3xl flex-col rounded-panel border border-border-strong bg-surface-raised shadow-lg console:h-full console:max-w-none console:rounded-none">
        <Dialog
          ref={dialog}
          data-cs="game-editor"
          aria-label={t`Edit ${title}`}
          className="flex min-h-0 flex-1 flex-col outline-none"
        >
          <Heading
            slot="title"
            data-part="title"
            className="px-6 pt-5 font-display text-xl font-semibold"
          >
            <Trans>Edit {title}</Trans>
          </Heading>
          <Tabs
            selectedKey={tab}
            onSelectionChange={(key: Key) => setTab(key as TabId)}
            className="flex min-h-0 flex-1 flex-col"
          >
            <TabList
              data-part="tabs"
              aria-label={t`Editor sections`}
              className="flex gap-2 border-b border-border px-6"
            >
              {TABS.map((id) => (
                <Tab
                  key={id}
                  id={id}
                  data-part="tab"
                  data-dirty={(id === "details" ? detailsDirty : dirtyTabs[id]) || undefined}
                  className="-mb-px cursor-pointer border-b-2 border-transparent px-3 py-2 text-sm text-fg-muted outline-none data-focus-visible:focus-ring data-hovered:text-fg data-selected:border-accent-alt data-selected:text-fg"
                >
                  {labels[id]}
                  {(id === "details" ? detailsDirty : dirtyTabs[id]) && (
                    <span aria-hidden className="ml-1 text-accent-alt">
                      •
                    </span>
                  )}
                </Tab>
              ))}
            </TabList>
            <div className="min-h-0 flex-1 overflow-y-auto px-6 py-4">
              {/* Every tab stays mounted, so switching keeps unsaved work. */}
              <TabPanel id="details" shouldForceMount className="data-inert:hidden">
                {state ? (
                  <DetailsTab
                    state={state}
                    draft={draft}
                    onDraft={onDraft}
                    errors={errors}
                    takeTheirs={(field) => {
                      rebase(field);
                      const next = { ...draft };
                      delete next[field];
                      setDraft(next);
                    }}
                    keepMine={rebase}
                  />
                ) : loaded.isPending ? (
                  <p className="text-sm text-fg-muted">{t`Loading…`}</p>
                ) : (
                  <p className="text-sm text-fg-muted">{t`This game is no longer in the library.`}</p>
                )}
              </TabPanel>
              <TabPanel id="artwork" shouldForceMount className="data-inert:hidden">
                {details && (
                  <ArtworkTab
                    gameId={gameId}
                    details={details}
                    onDirtyChange={tabDirty("artwork")}
                  />
                )}
              </TabPanel>
              <TabPanel id="actions" shouldForceMount className="data-inert:hidden">
                {details && (
                  <ActionsTab
                    gameId={gameId}
                    details={details}
                    onDirtyChange={tabDirty("actions")}
                  />
                )}
              </TabPanel>
              <TabPanel id="scripts" shouldForceMount className="data-inert:hidden">
                {details && (
                  <ScriptsTab
                    gameId={gameId}
                    details={details}
                    onDirtyChange={tabDirty("scripts")}
                  />
                )}
              </TabPanel>
              <TabPanel id="saves" shouldForceMount className="data-inert:hidden">
                {details && (
                  <SavesTab gameId={gameId} details={details} onDirtyChange={tabDirty("saves")} />
                )}
              </TabPanel>
            </div>
          </Tabs>
          <div
            data-part="footer"
            className="flex flex-wrap items-center gap-3 border-t border-border px-6 py-4"
          >
            <p role="status" data-part="status" className="mr-auto text-sm">
              {failure ? (
                <span className="text-danger">{failure}</span>
              ) : errors.length > 0 ? (
                <span className="text-danger">{t`Nothing was saved. Check the marked fields.`}</span>
              ) : save.isPending ? (
                <span className="text-fg-muted">{t`Saving…`}</span>
              ) : null}
            </p>
            <Button data-part="cancel" data-spatial="focusable" onPress={requestClose}>
              {detailsDirty || otherDirty ? <Trans>Cancel</Trans> : <Trans>Close</Trans>}
            </Button>
            {tab === "details" && (
              <Button
                data-part="save"
                data-spatial="focusable"
                variant="primary"
                isDisabled={!detailsDirty || save.isPending}
                onPress={submit}
              >
                {failure ? <Trans>Try again</Trans> : <Trans>Save</Trans>}
              </Button>
            )}
          </div>
        </Dialog>
      </Modal>
      {confirming && (
        <ModalOverlay
          isOpen
          isDismissable
          onOpenChange={(open) => {
            if (!open) setConfirming(false);
          }}
          className="fixed inset-0 z-50 grid place-items-center bg-surface-base/60 p-4"
        >
          <Modal className="w-full max-w-sm rounded-panel border border-border-strong bg-surface-raised p-6 shadow-lg">
            <Dialog
              role="alertdialog"
              data-cs="game-editor-discard"
              className="space-y-4 outline-none"
            >
              <Heading slot="title" className="font-display text-lg font-semibold">
                <Trans>Discard your changes?</Trans>
              </Heading>
              <p className="text-sm text-fg-muted">
                <Trans>Your unsaved changes to {title} will be lost.</Trans>
              </p>
              <div className="flex justify-end gap-2">
                <Button
                  data-spatial="focusable"
                  data-focus-default
                  onPress={() => setConfirming(false)}
                >
                  <Trans>Keep editing</Trans>
                </Button>
                <Button data-spatial="focusable" variant="danger" onPress={close}>
                  <Trans>Discard</Trans>
                </Button>
              </div>
            </Dialog>
          </Modal>
        </ModalOverlay>
      )}
    </ModalOverlay>
  );
}
