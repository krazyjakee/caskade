/** The catalogue entry of the story's component, shown beside the canvas so
 * theme authors see the selectors they may target. */
import { type AnatomyEntry, type AnatomyName, anatomy } from "@caskade/ui";

function Heading({ children }: { children: string }) {
  return <h3 className="mt-4 mb-1 font-mono text-[11px] text-fg-subtle uppercase">{children}</h3>;
}

function Code({ children }: { children: string }) {
  return <code className="font-mono text-xs text-accent-alt">{children}</code>;
}

export function AnatomyPanel({ name }: { name: AnatomyName }) {
  const entry: AnatomyEntry = anatomy[name];
  const parts = Object.entries(entry.parts);
  const states = Object.entries(entry.states);
  return (
    <aside
      aria-label={`Anatomy of ${name}`}
      className="w-80 shrink-0 self-start rounded-panel border border-border bg-surface-raised p-4 text-sm"
    >
      <Code>{`[data-cs="${name}"]`}</Code>
      <p className="mt-2 text-fg-muted">{entry.description}</p>
      <p className="mt-1 text-xs text-fg-subtle">
        {entry.rendered} · {entry.source}
      </p>
      {parts.length > 0 && (
        <>
          <Heading>Parts</Heading>
          <dl className="grid gap-1">
            {parts.map(([part, description]) => (
              <div key={part}>
                <dt>
                  <Code>{`[data-part="${part}"]`}</Code>
                  {entry.appOnly?.includes(part) && (
                    <span className="ml-2 text-xs text-fg-subtle">app only</span>
                  )}
                </dt>
                <dd className="text-xs text-fg-muted">{description}</dd>
              </div>
            ))}
          </dl>
        </>
      )}
      {states.length > 0 && (
        <>
          <Heading>States</Heading>
          <ul className="grid gap-1">
            {states.map(([attribute, values]) => (
              <li key={attribute}>
                <Code>{attribute}</Code>
                <span className="text-xs text-fg-muted"> {values.join(" · ")}</span>
              </li>
            ))}
          </ul>
        </>
      )}
      {entry.interaction.length > 0 && (
        <>
          <Heading>Interaction</Heading>
          <p className="font-mono text-xs text-fg-muted">{entry.interaction.join(" ")}</p>
        </>
      )}
      {(entry.contains?.length ?? 0) > 0 && (
        <>
          <Heading>Contains</Heading>
          <p className="font-mono text-xs text-fg-muted">{entry.contains?.join(" ")}</p>
        </>
      )}
      {(entry.slots?.length ?? 0) > 0 && (
        <>
          <Heading>Slots</Heading>
          <p className="font-mono text-xs text-fg-muted">{entry.slots?.join(" ")}</p>
        </>
      )}
    </aside>
  );
}
