import type { Meta } from "@storybook/react-vite";
import { interactions, parts, type Story } from "../../catalogue/stories";

const meta = { title: "Components/Select" } satisfies Meta;
export default meta;

export const Parts: Story = parts("select");
export const Interactions: Story = interactions("select");
