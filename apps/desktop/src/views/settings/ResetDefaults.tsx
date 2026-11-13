/** A settings page's Reset to defaults (#44): it names the group it resets,
 * lists the settings that would change (or says it can't tell, for a section
 * the core has no defaults for), and changes nothing until confirmed.
 * Only the fields a page lists go back to their defaults, so a page showing
 * one setting of another section (Start in, online lookups) resets that one
 * and leaves the rest of the section, such as whether first-run setup is
 * done, alone. */

import { Button } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { useMutation, useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Dialog, Heading } from "react-aria-components";
import { FailureNotice } from "../../components/FailureNotice";
import { ModalFrame } from "../../components/ModalFrame";
import { settingsKeys, useSettingsApi } from "./useSettings";

interface ResetField {
  key: string;
  /** The setting's name as its row shows it. */
  label: string;
}

export interface ResetPart {
  namespace: string;
  /** The settings this page shows from the section; only these are reset. */
  fields: readonly ResetField[];
  /** The page shows the whole section: the core resets all of it. */
  whole?: boolean;
}

export function ResetDefaults({
  group,
  label,
  parts,
  keeps,
  isDisabled,
}: {
  /** The group's name, e.g. "General". */
  group: string;
  /** The button's accessible name, e.g. "Reset general settings". */
  label: string;
  parts: readonly ResetPart[];
  /** What the reset leaves alone, where more than other pages' settings. */
  keeps?: string;
  isDisabled?: boolean;
}) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <Button
        variant="ghost"
        size="sm"
        onPress={() => setOpen(true)}
        isDisabled={isDisabled}
        aria-label={label}
      >
        <Trans>Reset to defaults…</Trans>
      </Button>
      {open && (
        <ResetDialog group={group} parts={parts} keeps={keeps} close={() => setOpen(false)} />
      )}
    </>
  );
}

interface Change {
  part: ResetPart;
  fields: readonly ResetField[];
  defaults: Record<string, unknown>;
}

/** Equal for the purpose of a reset; a missing value is the default. */
function same(current: unknown, fallback: unknown) {
  return current === undefined || JSON.stringify(current) === JSON.stringify(fallback);
}

function ResetDialog({
  group,
  parts,
  keeps,
  close,
}: {
  group: string;
  parts: readonly ResetPart[];
  keeps?: string;
  close: () => void;
}) {
  const { t } = useLingui();
  const api = useSettingsApi();
  const client = useQueryClient();
  const sections = useQuery({
    queryKey: ["settings", "sections"],
    queryFn: () => api.sections(),
    staleTime: 30_000,
  });
  const values = useQueries({
    queries: parts.map(({ namespace }) => ({
      queryKey: settingsKeys.section(namespace),
      queryFn: () => api.get(namespace),
      staleTime: 30_000,
    })),
  });
  const loading = sections.isPending || values.some((value) => value.isPending);
  const failure = sections.error ?? values.find((value) => value.error)?.error;
  const defaultsOf = ({ namespace }: ResetPart) => {
    const found = sections.data?.find((section) => section.namespace === namespace)?.defaults;
    return found !== null && typeof found === "object"
      ? (found as Record<string, unknown>)
      : undefined;
  };

  // What each part would change. Without its defaults a whole section can
  // still be reset by the core, though nothing says what that changes; a
  // field of another section can't.
  const changes = parts.flatMap((part, index): Change[] => {
    const value = values[index]?.data?.value;
    const defaults = defaultsOf(part);
    if (!defaults) return part.whole ? [{ part, fields: [], defaults: {} }] : [];
    if (!value) return [];
    const fields = part.fields.filter(({ key }) => !same(value[key], defaults[key]));
    return fields.length ? [{ part, fields, defaults }] : [];
  });
  const labels = changes.flatMap(({ fields }) => fields.map(({ label }) => label));
  const unseen = parts.filter((part) => !defaultsOf(part));
  const resetsUnseen = unseen.some((part) => part.whole);
  const leavesUnseen = unseen.some((part) => !part.whole);
  const canReset = !loading && !failure && changes.length > 0;

  const reset = useMutation({
    mutationFn: async () => {
      for (const { part, fields, defaults } of changes) {
        // The latest revision, as the section's own saves do.
        const latest = await api.get(part.namespace);
        const snapshot = part.whole
          ? await api.reset(part.namespace, latest.revision)
          : await api.set(
              part.namespace,
              {
                ...latest.value,
                ...Object.fromEntries(fields.map(({ key }) => [key, defaults[key]])),
              },
              latest.revision,
            );
        client.setQueryData(settingsKeys.section(part.namespace), snapshot);
      }
    },
    onSuccess: close,
  });

  return (
    <ModalFrame
      busy={reset.isPending}
      close={close}
      className="max-h-[85vh] w-full max-w-md overflow-y-auto rounded-panel border border-border-strong bg-surface-raised p-6 shadow-lg"
    >
      <Dialog role="alertdialog" className="space-y-4 outline-none">
        <Heading slot="title" className="font-display text-xl font-semibold">
          <Trans>Reset {group} settings?</Trans>
        </Heading>
        {loading ? (
          <p role="status" className="text-sm text-fg-muted">
            <Trans>Checking what would change…</Trans>
          </p>
        ) : failure ? (
          <FailureNotice
            variant="inline"
            error={failure}
            title={t`Caskade couldn't check what would change`}
            onRetry={() => {
              for (const query of [sections, ...values]) {
                if (query.error) void query.refetch();
              }
            }}
          />
        ) : (
          <>
            {labels.length > 0 && (
              <div className="space-y-2 text-sm">
                <p className="text-fg-muted">
                  <Trans>These settings go back to their defaults:</Trans>
                </p>
                <ul className="list-disc space-y-1 pl-5">
                  {labels.map((label) => (
                    <li key={label}>{label}</li>
                  ))}
                </ul>
              </div>
            )}
            {resetsUnseen && (
              <p className="text-sm text-fg-muted">
                {labels.length
                  ? t`Caskade can't tell what else would change. Reset also puts the rest of these settings back to their defaults.`
                  : t`Caskade can't tell what would change. Reset puts all of these settings back to their defaults.`}
              </p>
            )}
            {leavesUnseen && (
              <p className="text-sm text-fg-muted">
                {changes.length
                  ? t`Caskade can't tell what would change for some settings here, so those stay as they are.`
                  : t`Caskade can't tell what would change, so these settings can't be reset here.`}
              </p>
            )}
            {!labels.length && !unseen.length && (
              <p className="text-sm text-fg-muted">
                <Trans>Every setting here already uses its default.</Trans>
              </p>
            )}
          </>
        )}
        <p className="text-sm text-fg-muted">
          {keeps ?? t`Settings on other pages stay as they are.`}
        </p>
        {reset.error && <FailureNotice variant="inline" error={reset.error} />}
        <div className="flex justify-end gap-2">
          <Button
            data-spatial="focusable"
            data-focus-default
            isDisabled={reset.isPending}
            onPress={close}
          >
            {canReset || loading ? t`Cancel` : t`Close`}
          </Button>
          {/* Offered once it's known what it does: a greyed-out Reset gives
              no reason while the check runs. */}
          {canReset && (
            <Button
              data-spatial="focusable"
              variant="danger"
              isDisabled={reset.isPending}
              onPress={() => reset.mutate()}
            >
              {reset.isPending ? t`Resetting…` : t`Reset`}
            </Button>
          )}
        </div>
      </Dialog>
    </ModalFrame>
  );
}
