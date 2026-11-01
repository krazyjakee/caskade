import type { ReactNode } from "react";
import { PluginBoundary } from "./PluginBoundary";
import { type Registered, useContributions } from "./registry";
import type { PluginSlot as Slot } from "./types";

/** Renders a slot's contributions, each inside its plugin's error boundary. */
export function PluginSlot<S extends Slot>({
  slot,
  children,
}: {
  slot: S;
  children: (item: Registered<S>) => ReactNode;
}) {
  const items = useContributions(slot);
  return items.map((item) => (
    <PluginBoundary
      key={`${item.plugin.id}/${item.contribution.id}`}
      plugin={item.plugin}
      context={item.context}
    >
      {children(item)}
    </PluginBoundary>
  ));
}
