import type { Meta } from "@storybook/react-vite";
import { parts, type Story, states } from "../../catalogue/stories";

const meta = { title: "Components/Avatar" } satisfies Meta;
export default meta;

export const Parts: Story = parts("avatar");
export const States: Story = states("avatar");
