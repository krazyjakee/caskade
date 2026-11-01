/** Component stories generated from the anatomy catalogue: `Parts` renders
 * every part, `States` every value of every domain state the entry lists, and
 * `Interactions` each interaction attribute a prop can set. A new state value
 * in the catalogue shows up here without touching a story file. */
import { anatomy } from "@caskade/ui";
import type { StoryObj } from "@storybook/react-vite";
import type { ReactNode } from "react";
import { fixtures, type UiName } from "./fixtures";

export type Story = StoryObj;

function Gallery({ cells }: { cells: [label: string, content: ReactNode][] }) {
  return (
    <div className="flex flex-wrap items-start gap-6">
      {cells.map(([label, content]) => (
        <figure key={label} className="flex flex-col gap-2">
          <figcaption className="font-mono text-[11px] text-fg-subtle">{label}</figcaption>
          {content}
        </figure>
      ))}
    </div>
  );
}

/** The domain states the package renders (app-only ones are the app's). */
export function uiStates(name: UiName): [attribute: string, value: string][] {
  const entry = anatomy[name];
  const appOnly: readonly string[] = "appOnly" in entry ? entry.appOnly : [];
  return Object.entries(entry.states as Record<string, readonly string[]>)
    .filter(([attribute]) => !appOnly.includes(attribute))
    .flatMap(([attribute, values]) => values.map((value): [string, string] => [attribute, value]));
}

export const parts = (name: UiName): Story => ({
  parameters: { anatomy: name },
  render: () => fixtures[name].variants,
});

export const states = (name: UiName): Story => ({
  parameters: { anatomy: name },
  render: () => {
    const { state } = fixtures[name];
    if (!state) throw new Error(`${name} lists states but its fixture has no state render`);
    return (
      <Gallery
        cells={uiStates(name).map(([attribute, value]) => [
          `${attribute}="${value}"`,
          state(attribute, value),
        ])}
      />
    );
  },
});

export const interactions = (name: UiName): Story => ({
  parameters: { anatomy: name },
  render: () => (
    <Gallery
      cells={Object.entries(fixtures[name].interaction ?? {}).map(([attribute, element]) => [
        attribute,
        element,
      ])}
    />
  ),
});
