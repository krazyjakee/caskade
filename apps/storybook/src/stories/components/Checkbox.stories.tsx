import type { Meta } from "@storybook/react-vite";
import { interactions, parts, type Story } from "../../catalogue/stories";

const meta = { title: "Components/Checkbox" } satisfies Meta;
export default meta;

export const Parts: Story = parts("checkbox");
export const Interactions: Story = interactions("checkbox");
