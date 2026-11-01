import type { Meta } from "@storybook/react-vite";
import { parts, type Story, states } from "../../catalogue/stories";

const meta = { title: "Components/GameCard" } satisfies Meta;
export default meta;

export const Parts: Story = parts("game-card");
export const States: Story = states("game-card");
