import { composeStories } from "@storybook/react-vite";
import type { ComponentType } from "react";

type StoryModule = Parameters<typeof composeStories>[0];

const modules = import.meta.glob<StoryModule>("../**/*.stories.tsx", { eager: true });

/** Every story in the playground, composed with the project's annotations
 * (`setup.ts`): `[file › export, Story]`. */
export const allStories = Object.entries(modules).flatMap(([file, module]) =>
  Object.entries(composeStories(module)).map(([name, Story]): [string, ComponentType] => [
    `${file.replace("../stories/", "")} › ${name}`,
    Story as ComponentType,
  ]),
);
