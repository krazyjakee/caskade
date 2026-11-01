// A UI plugin built by @caskade/vite-plugin's test: React state, the SDK
// and an @caskade/ui component, all from the app at runtime.
import { definePlugin, usePlugin } from "@caskade/sdk";
import { Button } from "@caskade/ui";
import { useState } from "react";
import "./style.css";

function Counter() {
  const [clicks, setClicks] = useState(0);
  const { plugin } = usePlugin();
  return (
    <Button onPress={() => setClicks(clicks + 1)}>
      {plugin.id} clicked {clicks}
    </Button>
  );
}

export default definePlugin({
  activate(ctx) {
    ctx.slots.register("home.sections", { id: "counter", title: "Counter", component: Counter });
  },
});
