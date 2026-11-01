// Crashy (#134, test-only): every contribution throws while rendering, so
// the shell shows its placeholders instead.
import { definePlugin } from "@caskade/sdk";

function Broken(): never {
  throw new Error("crashy failed to render");
}

export default definePlugin({
  activate(ctx) {
    ctx.slots.register("views", { id: "broken", title: "Crashy", component: Broken });
    ctx.slots.register("home.sections", { id: "broken", title: "Crashy", component: Broken });
    ctx.slots.register("game.details.panels", { id: "broken", component: Broken });
  },
});
