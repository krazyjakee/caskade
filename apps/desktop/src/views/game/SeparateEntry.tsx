import { Button } from "@caskade/ui";
import { useLingui } from "@lingui/react/macro";
import { useSeparateEntry } from "../../api/igdb";

/** On a game with several entries (releases merged because IGDB counts
 * them as one game): moves this entry out into a game of its own, for
 * releases that aren't the same game. Matching never merges it back. */
export function SeparateEntry({ entryId, isRunning }: { entryId: string; isRunning: boolean }) {
  const { t } = useLingui();
  const separate = useSeparateEntry();
  return (
    <div className="flex shrink-0 flex-col items-end gap-1">
      <Button
        variant="secondary"
        isDisabled={isRunning || separate.isPending}
        onPress={() => separate.mutate(entryId)}
      >
        {separate.isPending
          ? t`Separating…`
          : t({
              message: "Separate",
              comment: "Button: moves this release out of the merged game into a game of its own",
            })}
      </Button>
      {separate.isError && (
        <p role="alert" className="text-xs text-danger">
          {separate.error.message}
        </p>
      )}
    </div>
  );
}
