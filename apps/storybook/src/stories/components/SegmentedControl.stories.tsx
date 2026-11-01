import type { Meta } from "@storybook/react-vite";
import { interactions, parts, type Story } from "../../catalogue/stories";

const meta = { title: "Components/SegmentedControl" } satisfies Meta;
export default meta;

export const Parts: Story = parts("segmented");
export const Interactions: Story = interactions("segmented");
