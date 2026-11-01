/** The app's own module instances that UI plugins import through the import
 * map (TECH-PLAN §4.4): one React tree, one SDK. The generated shims under
 * `/plugin-shims/` read them from here. */

import * as Sdk from "@caskade/sdk";
import * as Ui from "@caskade/ui";
import * as React from "react";
import * as JsxRuntime from "react/jsx-runtime";
import * as ReactDOM from "react-dom";
import * as ReactDOMClient from "react-dom/client";

export const sharedModules: Record<string, object> = {
  react: React,
  "react-dom": ReactDOM,
  "react-dom/client": ReactDOMClient,
  "react/jsx-runtime": JsxRuntime,
  "@caskade/sdk": Sdk,
  "@caskade/ui": Ui,
};

declare global {
  var __caskadeShared: Record<string, object> | undefined;
}

export function exposeSharedModules() {
  globalThis.__caskadeShared = sharedModules;
}
