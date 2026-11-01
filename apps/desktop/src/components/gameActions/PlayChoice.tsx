import { Select, Button as UiButton } from "@caskade/ui";
import { useLingui } from "@lingui/react/macro";
import { Wrench } from "lucide-react";
import { useActionChanges, useGameActions } from "../../api/actions";
import { useGameMenuHost } from "../gameMenu/GameMenuHost";

const ASK = "ask";

/** The game page's launch actions for the release Play starts (#178): which
 * play action Play starts, or that it asks each time, and the release's
 * tools. Nothing shows while the release has one play action and no tools. */
export function PlayChoice({
  gameId,
  title,
  entryId,
  isDisabled,
}: {
  gameId: string;
  title: string;
  entryId: string;
  isDisabled?: boolean;
}) {
  const { t } = useLingui();
  const release = useGameActions(gameId).data?.find((each) => each.entryId === entryId);
  const changes = useActionChanges(gameId);
  const host = useGameMenuHost();
  if (!release) return null;
  const play = release.actions.filter((action) => action.kind === "play");
  const tools = release.actions.filter((action) => action.kind === "utility");
  const settings = release.settings;
  const current = settings.ask
    ? ASK
    : (play.find((action) => action.id === settings.defaultAction) ?? play[0])?.id;
  return (
    <div data-cs="play-choice" className="flex flex-col gap-3">
      {play.length > 1 && current && (
        <Select
          label={t({
            message: "Play starts",
            comment: "Picks which of a release's launch actions Play starts",
          })}
          options={[
            ...play.map((action) => ({ id: action.id, label: action.name })),
            { id: ASK, label: t`Ask each time` },
          ]}
          value={current}
          onChange={(value) =>
            changes.settings.mutate({
              entryId,
              settings:
                value === ASK
                  ? { ...settings, ask: true }
                  : { ...settings, ask: false, defaultAction: value },
            })
          }
          isDisabled={isDisabled || changes.settings.isPending}
        />
      )}
      {changes.settings.isError && (
        <p role="alert" className="text-sm text-danger">
          {changes.settings.error.message}
        </p>
      )}
      {tools.length > 0 && host && (
        <div data-part="tools" className="flex flex-wrap gap-2">
          {tools.map((tool) => (
            <UiButton
              key={tool.id}
              data-spatial="focusable"
              icon={Wrench}
              onPress={() =>
                host.dialog(
                  { gameId, title },
                  { type: "tool", entryId, actionId: tool.id, name: tool.name },
                )
              }
            >
              {tool.name}
            </UiButton>
          ))}
        </div>
      )}
    </div>
  );
}
