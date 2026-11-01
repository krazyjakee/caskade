import type { Meta } from "@storybook/react-vite";
import { interactions, parts, type Story } from "../../catalogue/stories";

const meta = { title: "Components/Slider" } satisfies Meta;
export default meta;

export const Parts: Story = parts("slider");
export const Interactions: Story = interactions("slider");
