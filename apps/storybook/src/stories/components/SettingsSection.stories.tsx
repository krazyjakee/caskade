import type { Meta } from "@storybook/react-vite";
import { parts, type Story } from "../../catalogue/stories";

const meta = { title: "Components/SettingsSection" } satisfies Meta;
export default meta;

export const Parts: Story = parts("settings-section");
