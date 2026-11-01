import type { Meta } from "@storybook/react-vite";
import { parts, type Story, states } from "../../catalogue/stories";

const meta = { title: "Components/ErrorNotice" } satisfies Meta;
export default meta;

export const Parts: Story = parts("error-notice");
export const States: Story = states("error-notice");
