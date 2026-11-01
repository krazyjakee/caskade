import contract from "@caskade/tokens/manifest.json";
import type { Meta } from "@storybook/react-vite";
import type { Story } from "../../catalogue/stories";

/** The token contract, generated from `@caskade/tokens/manifest.json`: one
 * page per tier. Previews read the live `var()`, so they follow the toolbar's
 * theme, mode, scheme and effects; "Default" is the contract's own value. */
const meta = { title: "Tokens/Reference" } satisfies Meta;
export default meta;

type Token = (typeof contract)[number];

function Preview({ token }: { token: Token }) {
  const value = `var(${token.name})`;
  switch (token.type) {
    case "color":
      return (
        <span
          className="block size-8 rounded-control border border-border-strong"
          style={{ background: value }}
        />
      );
    case "dimension":
      return (
        <span
          className="block h-2 max-w-40 rounded-control bg-accent"
          style={{ width: `min(${value}, 10rem)` }}
        />
      );
    case "fontFamily":
      return <span style={{ fontFamily: value }}>Caskade 0123</span>;
    default:
      return null;
  }
}

function TokenTable({ tier }: { tier: Token["tier"] }) {
  const tokens = contract.filter((token) => token.tier === tier);
  return (
    <table className="w-full border-collapse text-left text-sm">
      <thead className="font-mono text-[11px] text-fg-subtle uppercase">
        <tr>
          <th className="py-2 pr-4">Token</th>
          <th className="py-2 pr-4">Type</th>
          <th className="py-2 pr-4">Preview</th>
          <th className="py-2 pr-4">Default</th>
          <th className="py-2">Description</th>
        </tr>
      </thead>
      <tbody>
        {tokens.map((token) => (
          <tr key={token.name} className="border-t border-border align-middle">
            <td className="py-2 pr-4">
              <code className="font-mono text-xs text-accent-alt">{token.name}</code>
              <div className="font-mono text-[11px] text-fg-subtle">{token.path}</div>
            </td>
            <td className="py-2 pr-4 font-mono text-xs text-fg-muted">{token.type}</td>
            <td className="py-2 pr-4">
              <Preview token={token} />
            </td>
            <td className="max-w-64 py-2 pr-4 font-mono text-xs break-all text-fg-muted">
              {token.resolved}
            </td>
            <td className="py-2 text-fg-muted">
              {"description" in token ? token.description : ""}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

export const Primitive: Story = { render: () => <TokenTable tier="primitive" /> };
export const Semantic: Story = { render: () => <TokenTable tier="semantic" /> };
export const Component: Story = { render: () => <TokenTable tier="component" /> };
