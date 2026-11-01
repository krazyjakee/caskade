import { Button } from "@caskade/ui";
import { useLingui } from "@lingui/react/macro";
import { useState } from "react";
import { type Key, Tab, TabList, TabPanel, Tabs } from "react-aria-components";
import { errorMessage } from "../api/reply";
import { useScreenTabs } from "../input";
import { PluginBoundary } from "./PluginBoundary";
import { PluginSlot } from "./PluginSlot";
import { type Registered, useContributions, usePluginRegistry } from "./registry";

/** Plugins' `game.actions`, and how to run one: a failure shows as the
 * plugin's notice. The game page's buttons and a game's menu use both. */
export function useGameActionContributions() {
  const { t } = useLingui();
  const actions = useContributions("game.actions");
  const registry = usePluginRegistry();
  const run = (action: Registered<"game.actions">, gameId: string) => {
    const { plugin, contribution } = action;
    Promise.resolve()
      .then(() => contribution.run(gameId))
      .catch((error) =>
        registry.notify(
          plugin,
          t`${{ action: contribution.title }} failed: ${{
            error: errorMessage(error),
          }}`,
          "error",
        ),
      );
  };
  return { actions, run };
}

/** Game details: plugin actions, tabs and panels. */
export function PluginGameDetails({ gameId }: { gameId: string }) {
  const { t } = useLingui();
  const { actions, run } = useGameActionContributions();
  const tabs = useContributions("game.details.tabs");
  const panels = useContributions("game.details.panels");
  const ids = tabs.map(({ plugin, contribution }) => `${plugin.id}/${contribution.id}`);
  const [chosen, setChosen] = useState<Key | null>(null);
  const selected = chosen !== null && ids.includes(String(chosen)) ? chosen : (ids[0] ?? null);
  // LB/RB switch the tabs (#150).
  useScreenTabs(ids, String(selected), setChosen);
  if (actions.length === 0 && tabs.length === 0 && panels.length === 0) return null;
  return (
    <section data-part="plugin-details" aria-label={t`From plugins`} className="mt-8 space-y-4">
      {actions.length > 0 && (
        <div data-part="plugin-actions" className="flex flex-wrap gap-2">
          {actions.map((action) => (
            <Button
              key={`${action.plugin.id}/${action.contribution.id}`}
              data-plugin={action.plugin.id}
              onPress={() => run(action, gameId)}
            >
              {action.contribution.title}
            </Button>
          ))}
        </div>
      )}
      {tabs.length > 0 && (
        <Tabs
          data-part="plugin-tabs"
          selectedKey={selected ?? undefined}
          onSelectionChange={setChosen}
        >
          <TabList aria-label={t`Plugin tabs`} className="flex gap-2 border-b border-border">
            {tabs.map(({ plugin, contribution }) => (
              <Tab
                key={`${plugin.id}/${contribution.id}`}
                id={`${plugin.id}/${contribution.id}`}
                className="-mb-px cursor-pointer border-b-2 border-transparent px-3 py-2 text-sm text-fg-muted outline-none data-focus-visible:focus-ring data-hovered:text-fg data-selected:border-accent-alt data-selected:text-fg"
              >
                {contribution.title}
              </Tab>
            ))}
          </TabList>
          {tabs.map(({ plugin, contribution: { id, component: Content }, context }) => (
            <TabPanel key={`${plugin.id}/${id}`} id={`${plugin.id}/${id}`} className="pt-4">
              <PluginBoundary plugin={plugin} context={context}>
                <Content gameId={gameId} />
              </PluginBoundary>
            </TabPanel>
          ))}
        </Tabs>
      )}
      <PluginSlot slot="game.details.panels">
        {({ contribution: { component: Panel } }) => <Panel gameId={gameId} />}
      </PluginSlot>
    </section>
  );
}

/** A library card's plugin badges and overlay; nothing when none exist. */
export function PluginCardExtras({ gameId }: { gameId: string }) {
  const badges = useContributions("game.card.badges");
  const overlays = useContributions("game.card.overlay");
  if (badges.length === 0 && overlays.length === 0) return null;
  return (
    <>
      {badges.length > 0 && (
        <div data-part="plugin-badges" className="absolute bottom-2 left-2 flex gap-1">
          <PluginSlot slot="game.card.badges">
            {({ contribution: { component: Badge } }) => <Badge gameId={gameId} />}
          </PluginSlot>
        </div>
      )}
      <PluginSlot slot="game.card.overlay">
        {({ contribution: { component: Overlay } }) => <Overlay gameId={gameId} />}
      </PluginSlot>
    </>
  );
}

/** Plugin badges inline in a library list row; overlays belong on card art. */
export function PluginRowBadges({ gameId }: { gameId: string }) {
  const badges = useContributions("game.card.badges");
  if (badges.length === 0) return null;
  return (
    <div data-part="plugin-badges" className="relative z-10 flex shrink-0 gap-1">
      <PluginSlot slot="game.card.badges">
        {({ contribution: { component: Badge } }) => <Badge gameId={gameId} />}
      </PluginSlot>
    </div>
  );
}

/** Dashboard shelves contributed by plugins. */
export function PluginHomeSections() {
  return (
    <PluginSlot slot="home.sections">
      {({ plugin, contribution: { title, component: Section } }) => (
        <section
          data-part="plugin-section"
          data-plugin={plugin.id}
          aria-label={title}
          className="mt-8"
        >
          <h2 className="mb-4 font-display text-xl font-semibold">{title}</h2>
          <Section />
        </section>
      )}
    </PluginSlot>
  );
}
