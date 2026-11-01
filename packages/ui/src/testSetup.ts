import { i18n } from "@lingui/core";
import { beforeEach } from "vitest";

// Components render in English, the source text, as they do without a
// catalog in the app.
beforeEach(() => {
  i18n.loadAndActivate({ locale: "en", messages: {} });
});
