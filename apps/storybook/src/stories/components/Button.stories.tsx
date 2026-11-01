import type { Meta } from "@storybook/react-vite";
import { interactions, parts, type Story, states } from "../../catalogue/stories";

const meta = { title: "Components/Button" } satisfies Meta;
export default meta;

export const Parts: Story = parts("button");
export const States: Story = states("button");
export const Interactions: Story = interactions("button");
