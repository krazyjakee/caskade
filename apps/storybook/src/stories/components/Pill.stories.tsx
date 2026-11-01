import type { Meta } from "@storybook/react-vite";
import { parts, type Story, states } from "../../catalogue/stories";

const meta = { title: "Components/Pill" } satisfies Meta;
export default meta;

export const Parts: Story = parts("pill");
export const States: Story = states("pill");
