// Caskade Pack's UI half (#219): one game action, in the language Caskade
// is in. Its texts come from locales/<lang>.po through `ctx.i18n`; Caskade
// activates the plugin again when the language changes, so the title
// registered here follows it.
import { definePlugin } from "@caskade/sdk";

export default definePlugin({
  activate(ctx) {
    const { t } = ctx.i18n;
    ctx.slots.register("game.actions", {
      id: "about-packs",
      title: t("About game packs"),
      run: () =>
        ctx.notify(
          t(
            "Caskade installs .cpk game packs: drop one on the library, or paste a link to its page on {host}.",
            {
              host: "games.example.com",
            },
          ),
        ),
    });
  },
});
