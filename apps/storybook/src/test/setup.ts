import { setProjectAnnotations } from "@storybook/react-vite";
import preview from "../preview";

// Portable stories get the playground's globals and decorator, as in Storybook.
setProjectAnnotations(preview);
