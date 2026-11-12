// Plugins whose backend draws their UI (plugin API 0.2, #109), in the
// webview until the native app replaces it (#112): each contribution is a
// tree of Caskade's components from the backend's `render`, drawn here with
// `@caskade/ui`. A thin, generic renderer: the native app's
// (`apps/native/src/plugins/`) is the one that stays.
import { definePlugin, type PluginDefinition, type PluginSlot, usePlugin } from "@caskade/sdk";
import { Button, SegmentedControl } from "@caskade/ui";
import { useEffect, useState } from "react";
import type { Context_Deserialize, Node_Serialize as UiNode, Value } from "../api/bindings";
import { useScreenTabs } from "../input";
import { useMode } from "../shell/console/mode";
import type { PluginUiApi, UiPlugin } from "./types";

type Act = (action: string, value: unknown) => void;

const gaps = { none: "gap-0", small: "gap-2", medium: "gap-4", large: "gap-6" } as const;
const tones = {
  neutral: "border-border bg-surface-base text-fg-muted",
  accent: "border-accent-alt/40 bg-accent-alt/10 text-accent-alt",
  success: "border-success/40 bg-success/10 text-success",
  warning: "border-warning/40 bg-warning/10 text-warning",
  danger: "border-danger/40 bg-danger/10 text-danger",
} as const;

function DrawnNode({ node, busy, act }: { node: UiNode; busy: boolean; act: Act }) {
  const children = (nodes: UiNode[]) =>
    nodes.map((child, index) => (
      // Trees are drawn again whole; a node's place is its identity.
      // biome-ignore lint/suspicious/noArrayIndexKey: see above
      <DrawnNode key={index} node={child} busy={busy} act={act} />
    ));
  switch (node.type) {
    case "text":
      return <span data-part="text">{node.text}</span>;
    case "heading":
      return node.level === 3 ? (
        <h3 data-part="heading" className="font-medium">
          {node.text}
        </h3>
      ) : (
        <h2 data-part="heading" className="font-display text-xl font-semibold">
          {node.text}
        </h2>
      );
    case "paragraph":
      return <p data-part="paragraph">{node.text}</p>;
    case "note":
      return (
        <p data-part="note" className="text-sm text-fg-muted">
          {node.text}
        </p>
      );
    case "alert":
      return (
        <p data-part="alert" role="alert" className="text-sm text-danger">
          {node.text}
        </p>
      );
    case "stack":
      return (
        <div
          data-part="stack"
          data-direction={node.direction}
          className={`flex ${node.direction === "row" ? "flex-row flex-wrap items-center" : "flex-col"} ${gaps[node.gap ?? "medium"]}`}
        >
          {children(node.children)}
        </div>
      );
    case "section":
      return (
        <section
          data-part="section"
          data-focus-section="plugin-section"
          aria-label={node.label}
          className="flex flex-col gap-4"
        >
          {children(node.children)}
        </section>
      );
    case "stats":
      return (
        <dl
          data-part="stats"
          aria-label={node.label ?? undefined}
          className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm"
        >
          {node.items.map((item, index) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: labels may repeat
            <div key={index} className="contents">
              <dt className="text-fg-muted">{item.label}</dt>
              <dd className="font-medium">{item.value}</dd>
            </div>
          ))}
        </dl>
      );
    case "tile":
      return (
        <div
          data-part="tile"
          className="flex flex-col gap-1 rounded-panel border border-border bg-surface-raised p-4"
        >
          <span data-part="value" className="font-display text-3xl font-semibold">
            {node.value}
          </span>
          <span data-part="label" className="text-sm text-fg-muted">
            {node.label}
          </span>
        </div>
      );
    case "bars": {
      const most = Math.max(...node.items.map((item) => item.value ?? 0), Number.MIN_VALUE);
      return (
        <ol data-part="bars" aria-label={node.label} className="flex flex-col gap-2">
          {node.items.map((item, index) => (
            <li
              // biome-ignore lint/suspicious/noArrayIndexKey: labels may repeat
              key={index}
              className="grid grid-cols-[minmax(0,12rem)_1fr_auto] items-center gap-3 text-sm"
            >
              <span className="truncate">{item.label}</span>
              <span aria-hidden className="h-2 overflow-hidden rounded-full bg-surface-base">
                <span
                  data-part="bar"
                  className="block h-full rounded-full bg-accent"
                  style={{ width: `${Math.min(100, ((item.value ?? 0) / most) * 100)}%` }}
                />
              </span>
              <span className="text-fg-muted tabular-nums">{item.text}</span>
            </li>
          ))}
        </ol>
      );
    }
    case "table":
      return (
        <table data-part="table" className="w-full border-collapse text-left text-sm">
          <caption className="mb-2 text-left font-medium">{node.caption}</caption>
          <thead>
            <tr>
              {node.columns.map((column, index) => (
                // biome-ignore lint/suspicious/noArrayIndexKey: columns may repeat
                <th key={index} scope="col" className="border-b border-border px-2 py-1">
                  {column}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {node.rows.map((row, index) => (
              // biome-ignore lint/suspicious/noArrayIndexKey: rows may repeat
              <tr key={index}>
                {row.map((cell, cellIndex) =>
                  node.rowHeaders && cellIndex === 0 ? (
                    // biome-ignore lint/suspicious/noArrayIndexKey: cells may repeat
                    <th key={cellIndex} scope="row" className="border-b border-border px-2 py-1">
                      {cell}
                    </th>
                  ) : (
                    // biome-ignore lint/suspicious/noArrayIndexKey: cells may repeat
                    <td key={cellIndex} className="border-b border-border px-2 py-1">
                      {cell}
                    </td>
                  ),
                )}
              </tr>
            ))}
          </tbody>
        </table>
      );
    case "list": {
      const items = node.items.map((item, index) => (
        // biome-ignore lint/suspicious/noArrayIndexKey: see above
        <li key={index}>
          <DrawnNode node={item} busy={busy} act={act} />
        </li>
      ));
      return node.ordered ? (
        <ol data-part="list" className="list-decimal pl-5">
          {items}
        </ol>
      ) : (
        <ul data-part="list" className="list-disc pl-5">
          {items}
        </ul>
      );
    }
    case "badge":
      return (
        <span
          data-part="badge"
          data-tone={node.tone}
          className={`inline-flex w-fit items-center rounded-control border px-2 py-0.5 text-xs font-medium ${tones[node.tone ?? "neutral"]}`}
        >
          {node.text}
        </span>
      );
    case "progress": {
      // Numbers cross as `number | null`; the host refused anything else.
      const share = Math.min(1, (node.value ?? 0) / (node.max ?? 1));
      return (
        <div
          data-part="progress"
          role="progressbar"
          aria-label={node.label}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={Math.round(share * 100)}
          className="h-1.5 overflow-hidden rounded-full bg-surface-base"
        >
          <div className="h-full bg-accent" style={{ width: `${share * 100}%` }} />
        </div>
      );
    }
    case "button":
      return (
        <Button
          data-part="button"
          data-spatial="focusable"
          size="sm"
          className="w-fit"
          variant={node.variant === "primary" ? "primary" : "secondary"}
          isDisabled={busy}
          onPress={() => act(node.action, node.value ?? null)}
        >
          {node.label}
        </Button>
      );
    case "segmented":
      return (
        <SegmentedControl
          data-part="segmented"
          aria-label={node.label}
          className="w-fit"
          isDisabled={busy}
          options={node.options.map((option) => ({ id: option.value, label: option.label }))}
          value={node.value}
          onChange={(value) => act(node.action, value)}
        />
      );
  }
}

/** The first segmented control in `nodes`, depth first. */
function firstSegmented(nodes: UiNode[]): Extract<UiNode, { type: "segmented" }> | null {
  for (const node of nodes) {
    if (node.type === "segmented") return node;
    const inner =
      node.type === "stack" || node.type === "section"
        ? firstSegmented(node.children)
        : node.type === "list"
          ? firstSegmented(node.items)
          : null;
    if (inner) return inner;
  }
  return null;
}

/** A tree, as `render` returned it. */
export function DrawnTree({ nodes, busy, act }: { nodes: UiNode[]; busy: boolean; act: Act }) {
  return (
    <div data-cs="plugin-ui" className="flex flex-col gap-4">
      {nodes.map((node, index) => (
        // biome-ignore lint/suspicious/noArrayIndexKey: see DrawnNode
        <DrawnNode key={index} node={node} busy={busy} act={act} />
      ))}
    </div>
  );
}

/** One contribution, drawn by its plugin's backend, with the state its
 * actions return. A failure throws to the plugin's boundary, which shows
 * its placeholder. */
function DrawnContribution({
  api,
  pluginId,
  slot,
  id,
  gameId,
}: {
  api: PluginUiApi;
  pluginId: string;
  slot: PluginSlot;
  id: string;
  gameId?: string;
}) {
  const plugin = usePlugin();
  const { mode } = useMode();
  const [state, setState] = useState<unknown>(null);
  const [drawn, setDrawn] = useState<{ nodes: UiNode[] } | { error: unknown } | null>(null);
  const [busy, setBusy] = useState(false);
  const [round, setRound] = useState(0);
  const context: Context_Deserialize = { mode, gameId: gameId ?? null, state: state as Value };
  // biome-ignore lint/correctness/useExhaustiveDependencies: drawn again by `round` too
  useEffect(() => {
    let current = true;
    api
      .render?.(pluginId, slot, id, context)
      .then((nodes) => current && setDrawn({ nodes }))
      .catch((error: unknown) => current && setDrawn({ error }));
    return () => {
      current = false;
    };
  }, [api, pluginId, slot, id, gameId, mode, state, round]);
  const act: Act = (action, value) => {
    setBusy(true);
    api
      .act?.(pluginId, slot, id, action, value, context)
      .then((outcome) => {
        if (outcome.notice) plugin.notify(outcome.notice);
        if (outcome.state !== undefined) setState(outcome.state);
        else setRound((count) => count + 1);
      })
      .catch((error: unknown) => {
        plugin.notify(error instanceof Error ? error.message : String(error));
        setRound((count) => count + 1);
      })
      .finally(() => setBusy(false));
  };
  // In a view, LB and RB move its first segmented control (#152).
  const segmented = drawn && "nodes" in drawn ? firstSegmented(drawn.nodes) : null;
  useScreenTabs(
    segmented?.options.map((option) => option.value) ?? [],
    segmented?.value ?? "",
    (value) => segmented && act(segmented.action, value),
    { enabled: slot === "views" && (segmented?.options.length ?? 0) > 1 },
  );
  if (drawn === null) return null;
  if ("error" in drawn) throw drawn.error;
  return (
    <div data-plugin={pluginId} className="contents">
      <DrawnTree nodes={drawn.nodes} busy={busy} act={act} />
    </div>
  );
}

const runs = new Set<PluginSlot>(["game.actions", "commands"]);

/** What the loader activates for a plugin whose backend draws its UI: each
 * declared contribution registered as a component that draws it, or as a
 * `run` that calls `act` with `run`. */
export function drawnDefinition(plugin: UiPlugin, api: PluginUiApi): PluginDefinition {
  return definePlugin({
    activate(ctx) {
      for (const { slot, id, title } of plugin.contributes) {
        if (runs.has(slot)) {
          ctx.slots.register(slot as "game.actions", {
            id,
            title: title ?? plugin.name,
            run: async (gameId?: string) => {
              const outcome = await api.act?.(plugin.id, slot, id, "run", null, {
                mode: "desktop",
                gameId: gameId ?? null,
                state: null,
              });
              if (outcome?.notice) ctx.notify(outcome.notice);
            },
          });
        } else {
          ctx.slots.register(slot as "views", {
            id,
            title: title ?? plugin.name,
            component: (props: { gameId?: string }) => (
              <DrawnContribution
                api={api}
                pluginId={plugin.id}
                slot={slot}
                id={id}
                gameId={props.gameId}
              />
            ),
          });
        }
      }
    },
  });
}
