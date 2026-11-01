/** The decorator behind the toolbar: applies the chosen theme to `cs.theme`
 * and the mode, colour scheme and reduce-effects attributes to `<html>`, the
 * way the app does, and shows the story's catalogue entry beside it. */
import type { AnatomyName } from "@caskade/ui";
import { type ReactNode, useLayoutEffect, useMemo } from "react";
import { AnatomyPanel } from "./catalogue/AnatomyPanel";
import { applyTheme, defaultScheme, themeStylesheet } from "./themes";
import { ThemeError } from "./themes/compile";

export interface PlaygroundGlobals {
  theme: string;
  mode: "desktop" | "console";
  scheme: "theme" | "dark" | "light";
  effects: "full" | "reduced";
  anatomy: "show" | "hide";
}

export function Playground({
  globals,
  anatomy,
  children,
}: {
  globals: PlaygroundGlobals;
  anatomy?: AnatomyName;
  children: ReactNode;
}) {
  const compiled = useMemo(() => {
    try {
      return { css: themeStylesheet(globals.theme) };
    } catch (error) {
      const problems = error instanceof ThemeError ? error.problems : [String(error)];
      return { css: "", problems };
    }
  }, [globals.theme]);
  const scheme = globals.scheme === "theme" ? defaultScheme(globals.theme) : globals.scheme;

  useLayoutEffect(() => {
    applyTheme(globals.theme, compiled.css);
  }, [globals.theme, compiled.css]);
  useLayoutEffect(() => {
    const root = document.documentElement;
    root.dataset.mode = globals.mode;
    root.dataset.colorScheme = scheme;
    root.style.colorScheme = scheme;
    if (globals.effects === "reduced") root.dataset.effects = "reduced";
    else delete root.dataset.effects;
  }, [globals.mode, globals.effects, scheme]);

  return (
    <div className="flex flex-col gap-4">
      {compiled.problems && (
        <div
          role="alert"
          data-playground="theme-problems"
          className="rounded-control border border-danger p-3 text-sm text-danger"
        >
          <p className="font-semibold">Theme {globals.theme} doesn't compile:</p>
          <ul className="mt-1 list-disc pl-5 font-mono text-xs">
            {compiled.problems.map((problem) => (
              <li key={problem}>{problem}</li>
            ))}
          </ul>
        </div>
      )}
      <div className="flex items-start gap-8">
        <div className="min-w-0 flex-1">{children}</div>
        {anatomy && globals.anatomy !== "hide" && <AnatomyPanel name={anatomy} />}
      </div>
    </div>
  );
}
