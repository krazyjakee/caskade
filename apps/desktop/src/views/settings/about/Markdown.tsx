import { Fragment, type ReactNode } from "react";
import { legalDocuments } from "./legal";
import { ExternalLink, SectionLink, textLink } from "./SectionLink";

/** The Markdown `docs/legal/` uses, and no more: headings, paragraphs, lists,
 * quotes and fenced code; links, bold and inline code. */
type Block =
  | { kind: "heading"; level: number; text: string }
  | { kind: "paragraph"; text: string }
  | { kind: "list"; items: string[] }
  | { kind: "quote"; text: string }
  | { kind: "code"; text: string };

const listItem = /^[-*]\s+/;
const blockStart = /^(#{1,6}\s|>|[-*]\s|```)/;

export function parseMarkdown(source: string): Block[] {
  const lines = source.replace(/\r\n/g, "\n").split("\n");
  const at = (i: number) => lines[i] ?? "";
  const blocks: Block[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = at(i);
    const heading = /^(#{1,6})\s+(.*)$/.exec(line);
    if (!line.trim()) {
      i++;
    } else if (heading) {
      blocks.push({ kind: "heading", level: heading[1]?.length ?? 1, text: heading[2] ?? "" });
      i++;
    } else if (line.startsWith("```")) {
      const body: string[] = [];
      for (i++; i < lines.length && !at(i).startsWith("```"); i++) body.push(at(i));
      i++;
      blocks.push({ kind: "code", text: body.join("\n") });
    } else if (line.startsWith(">")) {
      const body: string[] = [];
      for (; i < lines.length && at(i).startsWith(">"); i++) body.push(at(i).replace(/^>\s?/, ""));
      blocks.push({ kind: "quote", text: body.join(" ") });
    } else if (listItem.test(line)) {
      const items: string[] = [];
      for (; i < lines.length && (listItem.test(at(i)) || /^\s{2,}\S/.test(at(i))); i++) {
        if (listItem.test(at(i))) items.push(at(i).replace(listItem, ""));
        else items.push(`${items.pop() ?? ""} ${at(i).trim()}`);
      }
      blocks.push({ kind: "list", items });
    } else {
      const body: string[] = [];
      for (; i < lines.length && at(i).trim() && !blockStart.test(at(i)); i++)
        body.push(at(i).trim());
      blocks.push({ kind: "paragraph", text: body.join(" ") });
    }
  }
  return blocks;
}

const inlinePattern = /\[([^\]]+)\]\(([^)\s]+)\)|\*\*(.+?)\*\*|`([^`]+)`/g;

function DocLink({ href, children }: { href: string; children: ReactNode }) {
  const section = legalDocuments[href];
  if (section)
    return (
      <SectionLink section={section} className={textLink}>
        {children}
      </SectionLink>
    );
  if (/^https?:\/\//.test(href)) return <ExternalLink href={href}>{children}</ExternalLink>;
  // A path in the repository: not something the app can open.
  return <>{children}</>;
}

function Inline({ text }: { text: string }) {
  const parts: ReactNode[] = [];
  let last = 0;
  for (const match of text.matchAll(inlinePattern)) {
    const [whole, label, href, bold, code] = match;
    parts.push(text.slice(last, match.index));
    if (label !== undefined && href !== undefined)
      parts.push(
        <DocLink href={href}>
          <Inline text={label} />
        </DocLink>,
      );
    else if (bold !== undefined)
      parts.push(
        <strong className="font-semibold text-fg">
          <Inline text={bold} />
        </strong>,
      );
    else
      parts.push(
        <code className="rounded-sm bg-surface-overlay px-1 font-mono text-[0.9em] break-all">
          {code}
        </code>,
      );
    last = match.index + whole.length;
  }
  parts.push(text.slice(last));
  return (
    <>
      {parts.map((part, i) => (
        // Positions are stable: the text doesn't change while it is shown.
        // biome-ignore lint/suspicious/noArrayIndexKey: see above.
        <Fragment key={i}>{part}</Fragment>
      ))}
    </>
  );
}

const headings = ["h2", "h3", "h4", "h5", "h6"] as const;
const headingStyles = [
  "font-display text-2xl font-semibold tracking-wide text-fg",
  "mt-8 font-display text-lg font-semibold tracking-wide text-fg",
  "mt-6 text-base font-semibold text-fg",
];

/** A legal document from `docs/legal/`. Its `#` heading becomes a heading at
 * `level` (h3 by default), and the others follow below it. */
export function Markdown({ source, level = 3 }: { source: string; level?: number }) {
  return (
    <div className="flex flex-col gap-3 text-sm leading-relaxed text-fg-muted">
      {parseMarkdown(source).map((block, i) => {
        const key = `${block.kind}-${i}`;
        switch (block.kind) {
          case "heading": {
            const depth = Math.min(block.level - 1, headingStyles.length - 1);
            const Heading =
              headings[Math.min(level - 2 + block.level - 1, headings.length - 1)] ?? "h6";
            return (
              <Heading key={key} className={headingStyles[depth]}>
                <Inline text={block.text} />
              </Heading>
            );
          }
          case "paragraph":
            return (
              <p key={key}>
                <Inline text={block.text} />
              </p>
            );
          case "list":
            return (
              <ul key={key} className="flex list-disc flex-col gap-2 pl-5 marker:text-accent-alt">
                {block.items.map((item) => (
                  <li key={item}>
                    <Inline text={item} />
                  </li>
                ))}
              </ul>
            );
          case "quote":
            return (
              <blockquote
                key={key}
                className="rounded-control border border-warning/40 bg-warning/10 px-4 py-3 text-fg"
              >
                <Inline text={block.text} />
              </blockquote>
            );
          case "code":
            return (
              <pre
                key={key}
                className="rounded-control bg-surface-base/50 px-3 py-2 font-mono text-xs whitespace-pre-wrap break-words"
              >
                {block.text}
              </pre>
            );
        }
        return null;
      })}
    </div>
  );
}
