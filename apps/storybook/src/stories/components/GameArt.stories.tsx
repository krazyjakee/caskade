import type { Meta } from "@storybook/react-vite";
import { parts, type Story, states } from "../../catalogue/stories";

const meta = { title: "Components/GameArt" } satisfies Meta;
export default meta;

export const Parts: Story = parts("game-art");
export const States: Story = states("game-art");
