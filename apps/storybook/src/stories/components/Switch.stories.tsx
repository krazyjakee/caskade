import type { Meta } from "@storybook/react-vite";
import { interactions, parts, type Story, states } from "../../catalogue/stories";

const meta = { title: "Components/Switch" } satisfies Meta;
export default meta;

export const Parts: Story = parts("switch");
export const States: Story = states("switch");
export const Interactions: Story = interactions("switch");
