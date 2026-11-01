import type { Meta } from "@storybook/react-vite";
import { parts, type Story, states } from "../../catalogue/stories";

const meta = { title: "Components/StatusDot" } satisfies Meta;
export default meta;

export const Parts: Story = parts("status-dot");
export const States: Story = states("status-dot");
