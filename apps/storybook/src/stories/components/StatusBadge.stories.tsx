import type { Meta } from "@storybook/react-vite";
import { parts, type Story, states } from "../../catalogue/stories";

const meta = { title: "Components/StatusBadge" } satisfies Meta;
export default meta;

export const Parts: Story = parts("status-badge");
export const States: Story = states("status-badge");
