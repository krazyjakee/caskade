import { Button, SegmentedControl, Select, TextField } from "@caskade/ui";
import { msg, plural } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { ListChecks, Sparkles } from "lucide-react";
import { useState } from "react";
import { Dialog, Form, Heading, Modal, ModalOverlay } from "react-aria-components";
import { useSaveCollection } from "../../api/collections";
import { useLibraryPlatforms, useMatchCount } from "../../api/hooks";
import type { CollectionKind, CollectionSummary, LibraryQuery } from "../../api/types";
import { presentPlatform } from "../../views/library/platforms/catalog";
import { filterMessages, sortMessages } from "./rules";

const ANY_PLATFORM = "any";

const kinds = [
  { id: "manual", label: msg({ message: "Manual", context: "collection kind" }), icon: ListChecks },
  { id: "smart", label: msg({ message: "Smart", context: "collection kind" }), icon: Sparkles },
] as const;

/** Makes a collection, or edits one's name, description and rules. A new
 * one may start from `preset` (the Library's view, saved as a smart one). */
export function CollectionDialog({
  collection,
  preset,
  close,
  onSaved,
}: {
  collection?: CollectionSummary;
  preset?: CollectionKind;
  close: () => void;
  onSaved?: (id: string) => void;
}) {
  const { t } = useLingui();
  const start = collection?.kind ?? preset ?? { type: "manual" };
  const [name, setName] = useState(collection?.name ?? "");
  const [description, setDescription] = useState(collection?.description ?? "");
  const [type, setType] = useState<CollectionKind["type"]>(start.type);
  const [query, setQuery] = useState<LibraryQuery>(
    start.type === "smart" ? start.query : { filter: "installed", sort: "title" },
  );
  const smart = type === "smart";
  const platforms = useLibraryPlatforms("all", { enabled: smart });
  const matches = useMatchCount(query, { enabled: smart });
  const matching = matches.data;
  const save = useSaveCollection();
  const busy = save.isPending;
  const platformOptions = [
    { id: ANY_PLATFORM, label: t`Any platform` },
    ...(platforms.data ?? []).map((platform) => ({
      id: platform.name,
      label: presentPlatform(platform.name).title,
    })),
  ];
  // A saved platform the library no longer has stays choosable.
  if (query.platform && !platformOptions.some((option) => option.id === query.platform))
    platformOptions.push({ id: query.platform, label: presentPlatform(query.platform).title });
  return (
    <ModalOverlay
      isOpen
      isDismissable={!busy}
      onOpenChange={(open) => {
        if (!open && !busy) close();
      }}
      className="fixed inset-0 z-50 grid place-items-center bg-surface-base/80 p-4 backdrop-blur-sm"
    >
      <Modal className="max-h-[90vh] w-full max-w-lg overflow-auto rounded-panel border border-border-strong bg-surface-raised p-6 shadow-lg">
        <Dialog data-cs="collection-dialog" className="outline-none">
          <Heading slot="title" className="font-display text-xl font-semibold">
            {collection ? t`Edit ${{ name: collection.name }}` : t`New collection`}
          </Heading>
          <Form
            className="mt-4 space-y-4"
            onSubmit={(event) => {
              event.preventDefault();
              save.mutate(
                {
                  id: collection?.id,
                  name: name.trim(),
                  description: description.trim(),
                  kind: smart ? { type: "smart", query } : { type: "manual" },
                },
                {
                  onSuccess: (id) => {
                    onSaved?.(id);
                    close();
                  },
                },
              );
            }}
          >
            {!collection && (
              <SegmentedControl
                aria-label={t`Kind of collection`}
                options={kinds.map((kind) => ({ ...kind, label: t(kind.label) }))}
                value={type}
                onChange={setType}
              />
            )}
            <TextField
              label={t({ message: "Name", context: "collection" })}
              data-focus-default
              autoFocus
              value={name}
              onChange={setName}
              isRequired
              maxLength={200}
              isDisabled={busy}
            />
            <TextField
              label={t({ message: "Description", context: "collection" })}
              value={description}
              onChange={setDescription}
              maxLength={500}
              isDisabled={busy}
            />
            {smart ? (
              <fieldset data-part="rules" className="space-y-3">
                <legend className="mb-2 text-sm text-fg-muted">
                  <Trans>Games that match every rule, as the library changes</Trans>
                </legend>
                <div className="grid grid-cols-2 gap-3">
                  <Select
                    label={t({ message: "Games", context: "smart collection rule" })}
                    value={query.filter}
                    options={Object.entries(filterMessages).map(([id, label]) => ({
                      id: id as LibraryQuery["filter"],
                      label: t(label),
                    }))}
                    onChange={(filter) => setQuery({ ...query, filter })}
                  />
                  <Select
                    label={t({ message: "Platform", context: "smart collection rule" })}
                    value={query.platform ?? ANY_PLATFORM}
                    options={platformOptions}
                    onChange={(platform) =>
                      setQuery({
                        ...query,
                        platform: platform === ANY_PLATFORM ? undefined : platform,
                      })
                    }
                  />
                </div>
                <TextField
                  label={t({ message: "Title has", context: "smart collection rule" })}
                  placeholder={t`Any title`}
                  value={query.query ?? ""}
                  onChange={(text) => setQuery({ ...query, query: text || undefined })}
                  maxLength={200}
                />
                <Select
                  label={t`Sort by`}
                  value={query.sort}
                  options={Object.entries(sortMessages).map(([id, label]) => ({
                    id: id as LibraryQuery["sort"],
                    label: t(label),
                  }))}
                  onChange={(sort) => setQuery({ ...query, sort })}
                />
                <p data-part="matches" role="status" className="text-sm text-fg-muted">
                  {matching === undefined
                    ? t`Counting games…`
                    : t({
                        message: plural(matching, {
                          one: "# game matches now",
                          other: "# games match now",
                        }),
                      })}
                </p>
              </fieldset>
            ) : (
              !collection && (
                <p className="text-sm text-fg-muted">
                  <Trans>Add games from their menu, or from the collection's Add games.</Trans>
                </p>
              )
            )}
            {save.error && (
              <p role="alert" className="text-sm text-danger">
                {save.error.message}
              </p>
            )}
            <div className="flex justify-end gap-2">
              <Button onPress={close} isDisabled={busy}>
                <Trans>Cancel</Trans>
              </Button>
              <Button type="submit" variant="primary" isDisabled={!name.trim() || busy}>
                {busy
                  ? t`Saving…`
                  : collection
                    ? t`Save`
                    : t({ message: "Create", context: "collection" })}
              </Button>
            </div>
          </Form>
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}
