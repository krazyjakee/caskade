import { useLingui } from "@lingui/react/macro";
import { getRouteApi, Link } from "@tanstack/react-router";
import { isTauri } from "@tauri-apps/api/core";
import { lazy, Suspense, useEffect, useState } from "react";
import appConfig from "../../../../src-tauri/tauri.conf.json";
import { commands } from "../../../api/bindings";
import { useInputAction } from "../../../input";
import { AboutPage } from "./AboutPage";
import { type LegalSection, legalSection } from "./legal";
import { SectionLinkContext, type SectionLinkProps } from "./SectionLink";

// The notices and documents load with the Legal view, not with Settings.
const LegalView = lazy(() => import("./LegalView"));

const settingsRoute = getRouteApi("/settings");

/** In place, like the other sections' sub-views: Back leaves Settings from
 * About, and goes back to About from Legal. */
function RouterLink({ section, ...props }: SectionLinkProps) {
  return (
    <Link
      to="/settings"
      search={section ? { tab: "about", section } : { tab: "about" }}
      replace
      {...props}
    />
  );
}

/** The running app's version from its build metadata (Tauri's package info);
 * the bundled config's in the browser preview. */
function useAppVersion() {
  const [version, setVersion] = useState(appConfig.version);
  useEffect(() => {
    if (!isTauri()) return;
    let live = true;
    commands.backgroundStatus().then(
      (status) => live && setVersion(status.version),
      () => {},
    );
    return () => {
      live = false;
    };
  }, []);
  return version;
}

function useBuildLabel() {
  const { t } = useLingui();
  if (!isTauri()) return t`Browser preview`;
  return import.meta.env.DEV || import.meta.env.TAURI_ENV_DEBUG === "true"
    ? t`Debug build`
    : t`Release build`;
}

function Legal({ section }: { section: LegalSection }) {
  const { t } = useLingui();
  const navigate = settingsRoute.useNavigate();
  useInputAction("back", () => navigate({ search: { tab: "about" }, replace: true }), {
    label: t({ message: "About", context: "settings tab" }),
  });
  return (
    <Suspense fallback={<p className="text-sm text-fg-muted">{t`Loading…`}</p>}>
      <LegalView section={section} />
    </Suspense>
  );
}

/** Settings → About (#162). `?section=` opens a Legal section in its place. */
export function AboutSettings() {
  const build = useBuildLabel();
  const search = settingsRoute.useSearch();
  const section = legalSection(search.section);
  const version = useAppVersion();
  return (
    <SectionLinkContext value={RouterLink}>
      {section ? <Legal section={section} /> : <AboutPage version={version} build={build} />}
    </SectionLinkContext>
  );
}
