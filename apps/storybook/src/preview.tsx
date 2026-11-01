/** The playground's project annotations: the toolbar and its decorator.
 * `.storybook/preview.tsx` adds the stylesheets; the Vitest setup uses these
 * without them. */
import { locales, pseudoLocale, sourceLocale } from "@caskade/i18n";
import type { AnatomyName } from "@caskade/ui";
import type { Preview } from "@storybook/react-vite";
import { Locale } from "./Locale";
import { Playground, type PlaygroundGlobals } from "./Playground";
import { DEFAULT_THEME, NO_THEME, themes } from "./themes";

const preview: Preview = {
  parameters: {
    layout: "padded",
    backgrounds: { disable: true },
  },
  globalTypes: {
    theme: {
      description: "Theme: a folder under themes/, compiled into @layer cs.theme",
      toolbar: {
        title: "Theme",
        icon: "paintbrush",
        dynamicTitle: true,
        items: [
          ...themes.map((theme) => ({
            value: theme.manifest.id,
            title: theme.manifest.name,
            right: `themes/${theme.dir}`,
          })),
          { value: NO_THEME, title: "No theme", right: "contract defaults" },
        ],
      },
    },
    mode: {
      description: 'Mode: <html data-mode="desktop|console">',
      toolbar: {
        title: "Mode",
        icon: "browser",
        dynamicTitle: true,
        items: [
          { value: "desktop", title: "Desktop" },
          { value: "console", title: "Console" },
        ],
      },
    },
    scheme: {
      description: "Colour scheme: <html data-color-scheme>",
      toolbar: {
        title: "Scheme",
        icon: "contrast",
        dynamicTitle: true,
        items: [
          { value: "theme", title: "Theme default" },
          { value: "dark", title: "Dark" },
          { value: "light", title: "Light" },
        ],
      },
    },
    effects: {
      description: 'Reduce effects: <html data-effects="reduced">',
      toolbar: {
        title: "Effects",
        icon: "lightning",
        dynamicTitle: true,
        items: [
          { value: "full", title: "Full effects" },
          { value: "reduced", title: "Reduced effects" },
        ],
      },
    },
    locale: {
      description: "Language: @caskade/ui's catalog from locales/<lang>/ui.po",
      toolbar: {
        title: "Language",
        icon: "globe",
        dynamicTitle: true,
        items: locales.map((locale) => ({
          value: locale,
          title: languageName(locale),
          right: locale,
        })),
      },
    },
    anatomy: {
      description: "The component's anatomy catalogue entry beside the story",
      toolbar: {
        title: "Anatomy",
        icon: "eye",
        dynamicTitle: true,
        items: [
          { value: "show", title: "Show anatomy" },
          { value: "hide", title: "Hide anatomy" },
        ],
      },
    },
  },
  initialGlobals: {
    theme: DEFAULT_THEME,
    mode: "desktop",
    scheme: "theme",
    effects: "full",
    anatomy: "show",
    locale: sourceLocale,
  } satisfies PlaygroundGlobals & { locale: string },
  decorators: [
    (Story, context) => (
      <Locale locale={(context.globals.locale as string | undefined) ?? sourceLocale}>
        <Story />
      </Locale>
    ),
    (Story, context) => (
      <Playground
        globals={context.globals as PlaygroundGlobals}
        anatomy={context.parameters.anatomy as AnatomyName | undefined}
      >
        <Story />
      </Playground>
    ),
  ],
};

function languageName(locale: string): string {
  if (locale === pseudoLocale) return "Pseudo-locale";
  return new Intl.DisplayNames([locale], { type: "language" }).of(locale) ?? locale;
}

export default preview;
