/** What the macros compile `useLingui()` and `<Trans>` to (`runtimeConfigModule`
 * in `vite.ts`): Lingui's own, reading the nearest I18nProvider, and the
 * global `i18n` when there is none, as when a unit test renders a component
 * on its own. The app, first-run setup and Storybook render inside a provider. */
import { i18n } from "@lingui/core";
import {
  type I18nContext,
  I18nProvider,
  LinguiContext,
  Trans as LinguiTrans,
  type TransProps,
} from "@lingui/react";
import { useContext, useSyncExternalStore } from "react";

const globalContext = (): I18nContext => ({ i18n, _: i18n.t.bind(i18n) });
let snapshot = globalContext();
i18n.on("change", () => {
  snapshot = globalContext();
});
const subscribe = (onChange: () => void) => i18n.on("change", onChange);
const getSnapshot = () => snapshot;

export function useLingui(): I18nContext {
  const context = useContext(LinguiContext);
  const global = useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
  return context ?? global;
}

export function Trans(props: TransProps) {
  const context = useContext(LinguiContext);
  if (context) return <LinguiTrans {...props} />;
  return (
    <I18nProvider i18n={i18n}>
      <LinguiTrans {...props} />
    </I18nProvider>
  );
}
