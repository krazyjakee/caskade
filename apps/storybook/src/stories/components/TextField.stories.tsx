import type { Meta } from "@storybook/react-vite";
import { interactions, parts, type Story } from "../../catalogue/stories";

const meta = { title: "Components/TextField" } satisfies Meta;
export default meta;

export const Parts: Story = parts("text-field");
export const Interactions: Story = interactions("text-field");
