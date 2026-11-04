import { Button, Checkbox } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { Ban, Check } from "lucide-react";
import { useState } from "react";
import type { DiscoverySnapshot } from "../../../api/discovery";
import { useActionMenus } from "../../../components/ActionMenu";

export function ReviewList({
  snapshot,
  busy,
  decide,
}: {
  snapshot: DiscoverySnapshot;
  busy: boolean;
  decide: (ids: string[], accept: boolean) => void;
}) {
  const { t } = useLingui();
  const [selection, setSelection] = useState<Set<string>>(new Set());
  const selected = snapshot.review.filter((item) => selection.has(item.id)).map((item) => item.id);
  // A right click decides one result.
  const menus = useActionMenus();
  return (
    <section
      data-cs="discovery-review"
      aria-labelledby="discovery-review-heading"
      className="rounded-panel border border-border bg-surface-raised p-5"
    >
      <h2 id="discovery-review-heading" className="font-display text-lg font-semibold">
        <Trans>Review possible games</Trans>
      </h2>
      <p className="mt-1 text-sm text-fg-muted">
        <Trans>
          These results need your decision. Accept adds them to the library. Ignore saves a source
          exclusion for future scans.
        </Trans>
      </p>
      {snapshot.review.length === 0 ? (
        <p className="mt-4 text-sm text-fg-muted">
          <Trans>No games awaiting review.</Trans>
        </p>
      ) : (
        <>
          <div className="my-4 flex flex-wrap items-center gap-3">
            <Checkbox
              isSelected={selected.length === snapshot.review.length}
              isDisabled={busy}
              onChange={(checked) =>
                setSelection(new Set(checked ? snapshot.review.map((item) => item.id) : []))
              }
            >
              <Trans>Select all</Trans>
            </Checkbox>
            <Button
              size="sm"
              variant="primary"
              isDisabled={busy || selected.length === 0}
              onPress={() => decide(selected, true)}
            >
              {t({
                message: `Accept selected (${{ count: selected.length }})`,
                comment: "Accepts the checked discovery results; {count} is how many are checked",
              })}
            </Button>
            <Button
              size="sm"
              isDisabled={busy || selected.length === 0}
              onPress={() => decide(selected, false)}
            >
              <Trans>Ignore selected</Trans>
            </Button>
          </div>
          <ul className="space-y-3">
            {snapshot.review.map((item) => (
              <li
                key={item.id}
                {...menus.target(item.title, [
                  {
                    id: "accept",
                    label: t({ message: "Accept", context: "discovery review" }),
                    icon: Check,
                    description: t`Adds it to the library`,
                    isDisabled: busy,
                    onAction: () => decide([item.id], true),
                  },
                  {
                    id: "ignore",
                    label: t({ message: "Ignore", context: "discovery review" }),
                    icon: Ban,
                    description: t`Future scans skip it`,
                    isDisabled: busy,
                    onAction: () => decide([item.id], false),
                  },
                ])}
                className="rounded-control border border-border p-4"
              >
                <Checkbox
                  isSelected={selection.has(item.id)}
                  isDisabled={busy}
                  onChange={(checked) =>
                    setSelection((current) => {
                      const next = new Set(current);
                      if (checked) next.add(item.id);
                      else next.delete(item.id);
                      return next;
                    })
                  }
                >
                  {item.title}
                </Checkbox>
                <p className="mt-2 text-xs text-fg-muted">
                  <Trans comment="The discovery sources that found a game">
                    Sources: {item.sources.join(", ")}
                  </Trans>
                </p>
                {item.location && (
                  <p className="mt-1 break-all font-mono text-xs text-fg-muted">{item.location}</p>
                )}
                <ul
                  aria-label={t({
                    message: `Evidence for ${{ title: item.title }}`,
                    comment: "The reasons discovery thinks a result is a game",
                  })}
                  className="mt-2 space-y-1 text-xs text-fg-muted"
                >
                  {item.evidence.map((evidence) => (
                    <li key={evidence}>{evidence}</li>
                  ))}
                </ul>
              </li>
            ))}
          </ul>
          {menus.menu}
        </>
      )}
    </section>
  );
}
