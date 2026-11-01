import { AvatarPicker } from "@caskade/ui";
import type { Meta } from "@storybook/react-vite";
import { useState } from "react";
import { interactions, parts, type Story, states } from "../../catalogue/stories";

const meta = { title: "Components/AvatarPicker" } satisfies Meta;
export default meta;

export const Parts: Story = parts("avatar-picker");
export const States: Story = states("avatar-picker");
export const Interactions: Story = interactions("avatar-picker");

/** Click, arrow keys or Tab to choose; the chosen id shows below. */
export const Choose: Story = {
  parameters: { anatomy: "avatar-picker" },
  render: function Choose() {
    const [value, setValue] = useState("avatar:default");
    return (
      <div className="flex w-[40rem] flex-col gap-4">
        <AvatarPicker value={value} onChange={setValue} />
        <p className="font-mono text-xs text-fg-subtle">{value}</p>
      </div>
    );
  },
};
