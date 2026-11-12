/** The anatomy catalogue: the selectors themes and plugins
 * may target. Every catalogued component renders `data-cs="<name>"` on its
 * root, `data-part` on its parts and its domain state as the attributes in
 * `states`; React Aria adds the interaction attributes. Class names are not
 * API. `docs/theming/anatomy.md` is generated from this file. */

export interface AnatomyEntry {
  /** The components that render this root, and where they live. */
  rendered: string;
  source: "@caskade/ui" | "app" | "website";
  description: string;
  /** `data-part` names owned by this root → what each part is. */
  parts: Readonly<Record<string, string>>;
  /** Domain state on the root: attribute → the values it takes. */
  states: Readonly<Record<string, readonly string[]>>;
  /** React Aria interaction attributes on the root. */
  interaction: readonly string[];
  /** Parts and state attributes only the app's variants render (the
   * library's own card, the game page's hero). */
  appOnly?: readonly string[];
  /** Other catalogued roots that can render inside this one. */
  contains?: readonly string[];
  /** Parts that hold the caller's content, e.g. a row's controls: other
   * `data-part`s inside them belong to that content, not to this root. */
  slots?: readonly string[];
  /** The root's own content comes from the caller (a view, a panel), so
   * `data-part`s it doesn't list belong to that content. */
  container?: boolean;
}

const gameStatus = ["installed", "not-installed", "available", "emulated"] as const;
const press = ["data-hovered", "data-pressed", "data-focus-visible", "data-disabled"] as const;

export const anatomy = {
  website: {
    rendered: "apps/website/index.html (static GitHub Pages site)",
    source: "website",
    description: "The public landing page, sharing the app's tokens and branding.",
    parts: {
      skip: "Keyboard skip link to the main content.",
      header: "Brand, primary navigation, language dropdown and download link.",
      "main-nav": "The main page navigation links.",
      "header-actions": "The language dropdown and download link.",
      "language-trigger": "Native popover button with the current language flag and name.",
      "language-options": "Language links with flags; aria-current marks the current page.",
      brand: "Caskade's mark and name.",
      button: "A link styled as a button; data-variant is primary or small when set.",
      hero: "Introduction and desktop and console screenshots.",
      "hero-copy": "The main heading, introduction and actions.",
      eyebrow: "A short section introduction.",
      "status-dot": "A decorative accent beside the introduction.",
      intro: "The landing page introduction.",
      actions: "Download and source link groups, including platform installer choices.",
      "text-link": "A text link to a project resource.",
      "hero-note": "Free software and privacy summary.",
      screenshots: "Actual app captures with a sample library, linking to the full images.",
      "screenshot-main": "The cropped desktop screenshot and its link to the full capture.",
      "screenshot-console": "The cropped console screenshot and its link to the full capture.",
      "screenshot-label": "A mode label below a screenshot.",
      sources: "Examples of supported library sources.",
      video: "The introductory video and its direct YouTube link.",
      "video-player": "A responsive, lazy-loaded YouTube privacy-enhanced embed.",
      features: "The library, household and emulation feature overview.",
      "section-heading": "A section's introduction and heading.",
      "feature-grid": "The three feature descriptions.",
      feature: "One feature; data-feature is library, household or retro.",
      "feature-number": "A decorative feature index.",
      principles: "Local ownership, offline use, privacy and customisation.",
      "principle-list": "The three product principles.",
      downloads: "Platform status and download links.",
      "download-grid": "The three platform download panels.",
      platform: "A platform's status, formats and direct installer links.",
      "platform-title": "Platform name, icon and optional status badge.",
      badge: "The Linux reference-platform label.",
      "release-note": "Download instructions and the release archive link.",
      faq: "Native details disclosures with common questions.",
      closing: "The final download invitation.",
      footer: "Project documentation, source and licence links.",
    },
    states: {},
    interaction: [],
  },
  // ── @caskade/ui ──
  button: {
    rendered: "Button",
    source: "@caskade/ui",
    description: "A push button.",
    parts: { icon: "The leading icon." },
    states: {
      "data-variant": ["primary", "secondary", "ghost", "danger"],
      "data-size": ["sm", "md"],
    },
    interaction: [...press, "data-focused", "data-pending"],
  },
  checkbox: {
    rendered: "Checkbox",
    source: "@caskade/ui",
    description: "A checkbox with its label.",
    parts: { box: "The square.", mark: "The tick or dash inside the box.", label: "The label." },
    states: {},
    interaction: [...press, "data-selected", "data-indeterminate", "data-invalid", "data-readonly"],
  },
  segmented: {
    rendered: "SegmentedControl",
    source: "@caskade/ui",
    description: "One choice from a few options shown side by side.",
    parts: {
      segment: "An option; carries `data-selected`, `data-hovered` and `data-focus-visible`.",
      icon: "A segment's icon.",
    },
    states: {},
    interaction: ["data-disabled", "data-orientation"],
  },
  select: {
    rendered: "Select",
    source: "@caskade/ui",
    description: "A dropdown choice; its list opens in a `select-popover`.",
    parts: {
      label: "The visible label, when given.",
      trigger:
        "The button that opens the list; carries `data-hovered`, `data-pressed`, `data-focus-visible`.",
      value: "The selected option's text.",
      chevron: "The arrow on the trigger.",
    },
    states: {},
    interaction: [
      "data-open",
      "data-focused",
      "data-focus-visible",
      "data-disabled",
      "data-invalid",
    ],
    contains: ["select-popover"],
  },
  "select-popover": {
    rendered: "Select",
    source: "@caskade/ui",
    description: "The open list of a `select`, rendered in a portal.",
    parts: {
      option: "An option; carries `data-focused` and `data-selected`.",
      check: "The tick on the selected option.",
    },
    states: {},
    interaction: ["data-placement", "data-entering", "data-exiting"],
  },
  menu: {
    rendered: "MenuPopover, ContextMenu",
    source: "@caskade/ui",
    description:
      "A menu of actions, rendered in a portal: a button's dropdown, or a right click's menu at the pointer.",
    parts: {
      list: "The list of items.",
      item: 'An action; carries `data-focused` and `data-disabled`, and `data-tone="danger"` when it removes something.',
      icon: "An item's icon.",
      label: "An item's text.",
      description: "A second line under an item's text.",
      section: "A group of items.",
      "section-title": "A group's heading.",
      separator: "A line between groups.",
    },
    states: {},
    interaction: ["data-placement", "data-entering", "data-exiting"],
  },
  slider: {
    rendered: "Slider",
    source: "@caskade/ui",
    description: "A value on a track.",
    parts: {
      label: "The visible label, when given.",
      output: "The value readout.",
      track: "The rail; carries `data-hovered`.",
      fill: "The filled part of the rail.",
      thumb: "The handle; carries `data-dragging` and `data-focus-visible`.",
    },
    states: {},
    interaction: ["data-disabled", "data-orientation"],
  },
  switch: {
    rendered: "Switch",
    source: "@caskade/ui",
    description: "An on/off toggle.",
    parts: { track: "The pill.", thumb: "The knob.", label: "The label, when given." },
    states: { "data-size": ["sm", "md"] },
    interaction: [...press, "data-selected", "data-readonly"],
  },
  "text-field": {
    rendered: "TextField",
    source: "@caskade/ui",
    description: "A single-line text input.",
    parts: {
      label: "The visible label, when given.",
      control: "The box around the icon and input.",
      icon: "The leading icon.",
      input: "The `<input>`; carries `data-focused` and `data-hovered`.",
      action: "The control beside the input, e.g. a Browse… button.",
      description: "The help text.",
      error: "The validation message.",
    },
    states: {},
    interaction: ["data-disabled", "data-invalid", "data-readonly", "data-required"],
    slots: ["action"],
  },
  "settings-section": {
    rendered: "SettingsSection",
    source: "@caskade/ui",
    description: "A titled group of settings.",
    parts: {
      header: "The heading row.",
      icon: "The heading icon.",
      title: "The heading.",
      description: "The text under the heading.",
      aside: "Badges or actions at the end of the heading row.",
    },
    states: {},
    interaction: [],
    contains: ["setting-row"],
    slots: ["aside"],
    container: true,
  },
  "setting-row": {
    rendered: "SettingRow",
    source: "@caskade/ui",
    description: "One setting: its label on the left, its controls on the right.",
    parts: {
      icon: "The row icon.",
      label: "The setting's name.",
      description: "The text under the name.",
      controls: "The controls.",
    },
    states: {},
    interaction: [],
    slots: ["controls"],
  },
  "status-dot": {
    rendered: "StatusDot",
    source: "@caskade/ui",
    description: "A small coloured dot and label, e.g. Detected / Not found.",
    parts: { dot: "The dot.", label: "The text." },
    states: { "data-tone": ["ok", "off", "warning", "danger"] },
    interaction: [],
  },
  pill: {
    rendered: "Pill",
    source: "@caskade/ui",
    description: "A bordered chip with an icon, e.g. Off by default.",
    parts: { icon: "The icon.", label: "The text." },
    states: { "data-tone": ["warning", "danger", "accent"] },
    interaction: [],
  },
  "game-art": {
    rendered: "GameArt",
    source: "@caskade/ui",
    description: "A game's cover, or a blank placeholder when it has none.",
    parts: { image: "The artwork image." },
    states: { "data-source": ["image", "placeholder"] },
    interaction: [],
  },
  "status-badge": {
    rendered: "StatusBadge",
    source: "@caskade/ui",
    description: "A game's install status.",
    parts: { icon: "The status icon.", label: "The status text." },
    states: { "data-status": gameStatus },
    interaction: [],
  },
  "game-card": {
    rendered: "GameCard; the library's LibraryCard and the Web tab's tiles (app)",
    source: "@caskade/ui",
    description: "A game tile in a grid.",
    parts: {
      art: "The cover (a `game-art`).",
      badge: "The status (a `status-badge`).",
      title: "The game's title.",
      subtitle: "The genre; on LibraryCard, the platform unless it is a PC one.",
      meta: "The row of facts under the title.",
      playtime: "Time played.",
      size: "Size on disk.",
      "last-played": "When it was last played.",
      stores: "How many stores sell it.",
      owned: "Shown instead of stores when you own it.",
      favorite: "The favourite heart.",
      "storage-availability": "Shown when the managed copy is unavailable.",
      plugins: "Badges and overlays contributed by plugins.",
      select: "In selection mode, the check that shows whether the game is chosen.",
      "playing-elsewhere": "In console mode, the paired device the game is running on.",
      "played-on":
        "On Home's Recently Played, the other device the game was last played on, in place of the subtitle.",
      starting:
        "Starting…, over the art from a double click or Play until the game runs, the launch fails or a question stops it.",
    },
    states: {
      "data-status": gameStatus,
      "data-variant": ["library", "recent", "store", "web"],
      "data-storage-status": ["unavailable"],
      "data-tile": ["small", "large", "huge"],
      "data-selected": ["true"],
    },
    interaction: [],
    appOnly: [
      "favorite",
      "storage-availability",
      "plugins",
      "select",
      "playing-elsewhere",
      "played-on",
      "starting",
      "data-storage-status",
      "data-tile",
      "data-selected",
    ],
    contains: ["game-art", "status-badge"],
    slots: ["plugins"],
  },
  hero: {
    rendered: "Hero; the home and game pages' hero sections (app)",
    source: "@caskade/ui",
    description: "The large featured-game banner.",
    parts: {
      art: "The game's artwork (a `game-art`).",
      glow: "The coloured glow behind the content.",
      shade: "The gradient that keeps text readable.",
      fx: "The scanline and beam overlay.",
      content: "The text and actions.",
      eyebrow: "The small label above the title.",
      "live-dot": "The pulsing dot in the eyebrow.",
      title: "The game's title.",
      description: "The game's description.",
      meta: "Genre, size and playtime.",
      actions: "The buttons.",
      "primary-action": "The launch button group.",
      badge: "The status (a `status-badge`).",
      tags: "The game's tags.",
      website: "A link to the game's own page on the web (a web game's).",
      "played-on": "On Home, the other device the game was last played on (in the meta).",
    },
    states: {},
    interaction: [],
    appOnly: ["art", "tags", "website", "played-on"],
    contains: ["game-art", "status-badge"],
    slots: ["actions"],
  },
  shelf: {
    rendered: "Shelf",
    source: "@caskade/ui",
    description: "A titled row of cards.",
    parts: {
      header: "The heading row.",
      icon: "The heading icon.",
      title: "The heading.",
      subtitle: "The text beside the heading.",
      "view-all": "The View All button.",
      items: "The grid of cards.",
    },
    states: {},
    interaction: [],
    contains: ["game-card"],
    slots: ["items"],
  },
  avatar: {
    rendered: "Avatar",
    source: "@caskade/ui",
    description:
      "An account's picture: one of the shipped avatars, or the name's initials when its id is unknown or the image can't load.",
    parts: { image: "The avatar image.", initials: "The initials shown instead." },
    states: { "data-size": ["sm", "md", "lg", "xl"], "data-source": ["image", "initials"] },
    interaction: [],
  },
  "error-notice": {
    rendered: "ErrorNotice",
    source: "@caskade/ui",
    description:
      "A failure, shown the same way everywhere: what failed and why, what to do, and the technical details behind a toggle.",
    parts: {
      icon: "The alert icon.",
      title: "What couldn't be done, when given.",
      summary: "What failed and, when known, why.",
      hints: "What the user can do, one item each.",
      actions: "Recovery controls (e.g. Try again) and the details toggle.",
      "details-toggle": "Show details / Hide details; carries `aria-expanded`.",
      details: "The redacted technical details and the log reference.",
      reference: "The reference to find the failure in the log.",
    },
    states: {
      "data-variant": ["block", "inline"],
      "data-tone": ["danger", "warning"],
      "data-expanded": ["true", "false"],
    },
    interaction: [],
    contains: ["button"],
    slots: ["actions"],
  },
  "avatar-picker": {
    rendered: "AvatarPicker",
    source: "@caskade/ui",
    description: "A grid of the shipped avatars to choose one from (a radio group).",
    parts: {
      option:
        "An avatar to choose (an `avatar` inside); carries `data-selected`, `data-hovered` and `data-focus-visible`.",
      check: "The tick on the chosen avatar.",
    },
    states: { "data-size": ["md", "lg", "xl"] },
    interaction: ["data-disabled", "data-orientation"],
    contains: ["avatar"],
  },

  // ── App shell ──
  "app-shell": {
    rendered: "AppShell (app)",
    source: "app",
    description: "The window: sidebar, topbar, current view and persistent status bar.",
    parts: { main: "The column right of the sidebar." },
    states: {},
    interaction: [],
    contains: ["backdrop", "sidebar", "topbar", "view", "status-bar"],
    container: true,
  },
  "status-bar": {
    rendered: "StatusBar (app)",
    source: "app",
    description:
      "Persistent background activity, application version and an available Caskade update. IGDB and Hasheous are credited in Settings → About (`about`).",
    parts: {
      spinner: "The activity spinner; hidden while idle.",
      activity: "A concise description of current work or queue status.",
      version: "The application version.",
      update:
        "A link to Settings → General once a check finds a new Caskade version; `data-status` is `available`, or `ready` once it is installed and waits for a restart.",
      sync: "With paired devices (#231): how many are in reach and the best way one is reached (a `peer-link`), linked to Settings → Devices; `data-status` is `lan`, `direct`, `relay`, `offline` or `syncing`.",
    },
    states: {},
    interaction: [],
  },
  backdrop: {
    rendered: "AppShell (app)",
    source: "app",
    description: "The decorative layer behind everything.",
    parts: {},
    states: {},
    interaction: [],
  },
  onboarding: {
    rendered: "Onboarding (app)",
    source: "app",
    description:
      "First-run setup, shown in place of the app shell: a welcome, the questions (`language`, `folder`, `sources`, `mode`, and `account`, the admin's name, avatar and PIN), then the work they start (`setup`). Things arriving use `animate-rise`; the running task's sweep pauses with `--cs-motion-ambient`.",
    parts: {
      titlebar: "The row at the top that moves the window.",
      sound: "Turns setup's sounds on and off.",
      skip: "Skips setup, keeping the defaults.",
      main: "The current step.",
      welcome: "The first screen.",
      logo: "The logo on the welcome screen, with its glow.",
      start: "Get started.",
      question: "A question; `data-question` names it.",
      card: "The games folder, its free space and its Change button.",
      choose: "Chooses another games folder.",
      source: "Something to look for; `data-status` is `on` or `off`.",
      mode: "Desktop or console mode; React Aria's `data-selected` marks the choice.",
      language: "A language, or System; React Aria's `data-selected` marks the choice.",
      next: "Continue, Set up, or Open my library.",
      progress: "The step dots; each has `data-status` `active`, `done` or `next`.",
      setup: "The work being done; `data-status` is `running` or `done`.",
      covers: "Covers of the games found so far, behind the list.",
      count: "How many games the library has.",
      tasks: "The list of tasks.",
      task: "One task; `data-task` names it, `data-status` is `waiting`, `running`, `done`, `warning` or `failed`.",
    },
    states: {
      "data-step": ["welcome", "language", "folder", "sources", "mode", "account", "setup"],
    },
    interaction: [],
    contains: [
      "backdrop",
      "window-controls",
      "button",
      "switch",
      "checkbox",
      "text-field",
      "game-art",
      "avatar-picker",
    ],
  },
  sidebar: {
    rendered: "Sidebar (app)",
    source: "app",
    description: "The navigation column.",
    parts: {
      brand: "The logo and name.",
      nav: "A group of links.",
      "nav-item": 'A link; the current one has `data-status="active"`.',
      icon: "A link's icon.",
      label: "A link's text.",
      divider: "The rule between the groups.",
      "sub-nav":
        "The links under a link: the Library's filters, open (`data-open`) while the Library is.",
      "sub-item": 'A filter; the current one has `data-status="active"`.',
      count: "How many games a filter has.",
      "plugin-nav": "The links to plugin views.",
    },
    states: {},
    interaction: [],
    slots: ["plugin-nav"],
  },
  topbar: {
    rendered: "Topbar (app)",
    source: "app",
    description:
      "The bar above the view: search, quick actions and the window controls. It is the window's title bar: dragging it moves the window.",
    parts: {
      actions: "The items at the end of the bar.",
      activity: "The link to Activity.",
      "job-count": "The number of running jobs; `data-status` is `active` or `needs-user`.",
      "notification-count": "The number of unread notifications, apart from the jobs.",
      avatar: "The signed-in account's button, which opens the account menu (an `avatar` inside).",
      console:
        "Enters console mode, where Caskade draws no window controls (macOS, the browser preview).",
    },
    states: {},
    interaction: [],
    contains: ["search", "window-controls", "avatar"],
    slots: ["actions"],
  },
  "window-controls": {
    rendered: "WindowControls (app)",
    source: "app",
    description:
      "Minimize, maximize and close at the end of the topbar (Windows and Linux; macOS keeps its traffic lights). Each button carries `data-hovered` and `data-pressed`.",
    parts: {
      minimize: "Minimizes the window.",
      console: "Enters console mode (fullscreen, for a controller).",
      maximize: "Maximizes the window, or restores it when maximized.",
      close: "Closes the window.",
    },
    states: { "data-status": ["restored", "maximized"] },
    interaction: [],
  },
  "view-error": {
    rendered: "ViewError, AppCrash (app)",
    source: "app",
    description:
      "Shown in place of a view that failed to render, with the shell around it; or in place of the whole window when the shell itself failed, with the window controls.",
    parts: {
      title: "The heading.",
      message: "The error's text.",
      actions: "Try again and Open the library, or Reload.",
    },
    states: {},
    interaction: [],
  },
  "console-shell": {
    rendered: "ConsoleShell (app)",
    source: "app",
    description:
      'Console mode\'s window (`[data-mode="console"]` on the root): the console sidebar, top bar and view, and the button-hint bar. `data-cursor` is `hidden` while the pointer rests.',
    parts: { main: "The column right of the sidebar." },
    states: {},
    interaction: [],
    contains: ["backdrop", "console-sidebar", "console-topbar", "view", "hint-bar"],
    container: true,
  },
  "console-sidebar": {
    rendered: "ConsoleSidebar (app)",
    source: "app",
    description: "Console mode's navigation: big targets.",
    parts: {
      brand: "The logo and name.",
      nav: "The links.",
      "nav-item": 'A link; the current one has `data-status="active"`.',
      icon: "A link's icon.",
      label: "A link's text.",
      "sub-nav":
        "The links under a link: the Library's filters, open (`data-open`) while the Library is.",
      "sub-item": 'A filter; the current one has `data-status="active"`.',
      count: "How many games a filter has.",
      "plugin-nav": "Plugin views that show in console mode.",
      leave: "Goes back to desktop mode.",
    },
    states: {},
    interaction: [],
    slots: ["plugin-nav"],
  },
  "console-topbar": {
    rendered: "ConsoleTopbar (app)",
    source: "app",
    description: "Console mode's top bar: search and activity.",
    parts: {
      actions: "The items at the end of the bar.",
      activity: "The link to Activity.",
      "job-count": "The number of running jobs; `data-status` is `active` or `needs-user`.",
      "notification-count": "The number of unread notifications, apart from the jobs.",
      avatar: "The signed-in account's button, which opens the account menu (an `avatar` inside).",
    },
    states: {},
    interaction: [],
    contains: ["search", "select", "avatar"],
  },
  notifications: {
    rendered: "NotificationFeed (app)",
    source: "app",
    description:
      "Activity's notification feed (#198): game additions in hourly batches, failed jobs, updates on offer and plugins turned off, newest change first.",
    parts: {
      toolbar: "The filters and Mark all read.",
      filter: "All or Unread; the current one has `data-selected`.",
      "mark-all": "Marks read every item shown so far.",
      empty: "Shown when the feed (or its unread filter) is empty.",
      pages: "Newer and Older, when the feed has more than a page.",
    },
    states: {},
    interaction: [],
    contains: ["notification", "button"],
  },
  notification: {
    rendered: "NotificationFeed (app)",
    source: "app",
    description:
      "One item of the notification feed. `data-category` is what it is about; `data-resolved` is set once its problem has cleared.",
    parts: {
      indicator: "The unread dot.",
      meta: "Its category and time.",
      time: "When: a batch's first to last addition, else its last change.",
      title: "The heading.",
      body: "Details: a batch's sources, a failure's error.",
      actions: "Open (the list, the game or the settings it is about) and Mark read or unread.",
      members: "What it is about, once opened: a batch's games, the updates on offer.",
      member:
        "One of them; `data-status` is `present`, `merged` (it links to the game it joined), `removed` or `other`.",
    },
    states: {
      "data-status": ["unread", "read"],
      "data-severity": ["info", "warning", "error"],
      "data-category": [
        "gamesAdded",
        "jobFailed",
        "updatesAvailable",
        "pluginDisabled",
        "achievementsUnlocked",
      ],
      "data-resolved": ["true"],
    },
    interaction: [],
    contains: ["button"],
  },
  "hint-bar": {
    rendered: "HintBar (app)",
    source: "app",
    description:
      "What each controller button does where focus is. `data-glyphs` is the controller family in hand: `xbox`, `playstation`, `nintendo`, `generic`, or `keyboard`.",
    parts: {
      hint: "One button and what it does; `data-action` is the action (`confirm`, `back`, `tab.next`, …).",
      glyph:
        "The button's label; face buttons carry `data-tone` (`installed`, `danger`, `accent`, `warning`).",
      label: "What the button does.",
      clock: "The time.",
    },
    states: {},
    interaction: [],
  },
  "game-curtain": {
    rendered: "GameCurtain (app)",
    source: "app",
    description:
      "Console mode's black screen behind a running game: it fades in when a game starts, so the game's window takes over without Caskade minimizing, and fades out when the last game exits. Always present in console mode; `data-state` is `covered` while it is down.",
    parts: {
      title: "The game's title.",
      hint: "The guide button, which opens the quick menu over it.",
    },
    states: { "data-state": ["covered", "open"] },
    interaction: [],
  },
  "shelf-row": {
    rendered: "ShelfRow (app)",
    source: "app",
    description: "A row of game cards that pages with LB/RB (Home's shelves).",
    parts: {
      header: "The title row.",
      icon: "The shelf's icon.",
      title: "The shelf's name.",
      pager: "The LB/RB buttons, in console mode.",
      "view-all": "The link to every game in the shelf.",
      items: "The row of cards.",
    },
    states: {},
    interaction: [],
    slots: ["items"],
  },
  search: {
    rendered: "SearchBox (app)",
    source: "app",
    description: "The game search; suggestions open in a `search-popover`.",
    parts: {
      control: "The box around the input; carries `data-focus-within`.",
      icon: "The magnifier.",
      input: "The `<input>`.",
      shortcut:
        "The shortcut that focuses it: Ctrl K, or in console mode the controller's search button.",
    },
    states: {},
    interaction: ["data-open", "data-disabled"],
    contains: ["search-popover"],
  },
  "search-popover": {
    rendered: "SearchBox (app)",
    source: "app",
    description: "The search suggestions, rendered in a portal.",
    parts: {
      suggestion: "A suggested game; carries `data-focused`.",
      "see-all": "The item that opens every result in the library.",
    },
    states: {},
    interaction: ["data-placement", "data-entering", "data-exiting"],
    contains: ["game-art", "status-badge"],
  },
  view: {
    rendered: "AppShell (app)",
    source: "app",
    description: "The scrolling area that shows the current page.",
    parts: {},
    states: {},
    interaction: [],
    container: true,
  },
  "settings-nav": {
    rendered: "SettingsView (app)",
    source: "app",
    description:
      "Settings' sections: a vertical tab list beside the open section's panel, in desktop and console modes, grouped under headings, with the settings search above it. It stays in view while the panel scrolls, and scrolls on its own when the sections don't fit.",
    parts: {
      group: "A group's heading, before its sections (App, Library, …); not a stop for focus.",
      tab: "A section; the open one carries `data-selected`, and each `data-hovered`, `data-pressed` and `data-focus-visible`.",
      icon: "A section's icon.",
      label: "A section's name; long names wrap.",
    },
    states: {},
    interaction: ["data-orientation"],
  },
  about: {
    rendered: "AboutPage (app)",
    source: "app",
    description:
      "Settings → About: the IGDB and Hasheous credits first, side by side, then the app, its version and build, project links, and the links into `legal`.",
    parts: {
      title: "The page heading.",
      credits: "The list of data credits, directly under the heading.",
      credit: "One credit (IGDB, Hasheous): a link to the service, each with the same weight.",
      details: "The app's name, logo and short description.",
      version: "The version and build, from the app's build metadata.",
      links: "Links to the project page, issue tracker and releases.",
      "legal-links": "Links to each section of the Legal view.",
    },
    states: {},
    interaction: [],
  },
  legal: {
    rendered: "LegalView (app)",
    source: "app",
    description:
      "Settings → About → Legal: licences, open-source packages, terms and privacy policy, bundled for offline reading. One section shows at a time, at `/settings?tab=about&section=<id>`.",
    parts: {
      back: "The link back to About.",
      title: "The view's heading.",
      nav: "The section links.",
      "section-link": "A section; the open one carries `aria-current`.",
      document:
        "A long text (a policy, a licence group's packages and texts): focusable, so keys and a pad scroll it.",
    },
    states: {},
    interaction: [],
  },
  "system-stats": {
    rendered: "StatisticsDashboard (app)",
    source: "app",
    description:
      "Settings → Statistics: this machine's CPU, GPUs, memory, network and storage capacity, live, as the same cards in desktop and console modes. `data-status` says whether readings are coming in.",
    parts: {
      title: "The section heading.",
      status: "The line under it: live, waiting, not updating or unavailable, in words.",
      card: "One metric (`data-metric`: `cpu`, `memory`, `gpu`, `network`, `storage`); focusable, so keys and a pad reach every card.",
      gauge: "A utilization gauge (`role=meter`) with its percentage written inside.",
      graph: "A recent-history graph; a missed sample is a gap in its line.",
      unavailable: "Why a metric has no value: unsupported, no device or a failed sample.",
      interface:
        "A network interface and its rates; `data-kind` is `physical`, `virtual` or `loopback`.",
      volume: "A storage volume: its name, mount points and capacity.",
      capacity: "A volume's used-space bar (`role=meter`).",
    },
    states: { "data-status": ["loading", "live", "stale", "error"] },
    interaction: [],
  },
  panel: {
    rendered: "Collection, Collections and Emulation views (app)",
    source: "app",
    description: "A framed panel inside a view.",
    parts: {},
    states: {},
    interaction: [],
    container: true,
  },
  "collection-card": {
    rendered: "CollectionCard (app)",
    source: "app",
    description: "A collection tile.",
    parts: {
      art: "The mosaic of its games.",
      badge: "Smart or manual.",
      pin: "Shown on pinned collections.",
      count: "How many games it holds.",
      title: "The collection's name.",
      subtitle: "Its description.",
      meta: "Playtime and when it changed.",
    },
    states: { "data-kind": ["smart", "manual"] },
    interaction: [],
  },
  "platform-card": {
    rendered: "PlatformGrid (app)",
    source: "app",
    description: "A platform in the library's breakdown of emulated games; opens its games.",
    parts: {
      art: "The hardware render on its glow.",
      title: "The platform's short name.",
      subtitle: "Its maker and kind of machine.",
      count: "How many games it has.",
      meta: "Playtime and last play.",
      playtime: "Time played on it.",
      "last-played": "When one of its games was last played.",
    },
    states: { "data-art": ["platform", "generic"] },
    interaction: [],
  },
  "game-row": {
    rendered: "LibraryRow (app)",
    source: "app",
    description: "A game in the library's compact table (the list layout).",
    parts: {
      icon: "The game's icon, or its initial on generated art.",
      title: "The game's title; links to the game and covers the row.",
      favorite: "The favourite heart.",
      "storage-availability": "Shown when the managed copy is unavailable.",
      plugins: "Badges contributed by plugins.",
      badge: "The status (a `status-badge`).",
      "last-played": "When it was last played.",
      playtime: "Time played.",
      size: "Size on disk.",
      actions: "The actions menu button.",
      select: "In selection mode, the check that shows whether the game is chosen.",
      starting:
        "Starting…, after the title from a double click or Play until the game runs, the launch fails or a question stops it.",
    },
    states: {
      "data-status": ["installed", "not-installed", "emulated"],
      "data-storage-status": ["unavailable"],
      "data-selected": ["true"],
    },
    interaction: [],
    contains: ["status-badge"],
    slots: ["plugins"],
  },
  "alphabet-bar": {
    rendered: "AlphabetBar (app)",
    source: "app",
    description:
      "The letters beside a long library sorted by title; each jumps to its games, and a drag jumps letter by letter.",
    parts: {
      letter:
        "One initial (`#` for numbers and symbols); `data-selected` on the letter on screen, `data-disabled` when no game starts with it.",
    },
    states: {},
    interaction: [],
  },
  "quick-look": {
    rendered: "QuickLook (app)",
    source: "app",
    description:
      "A game at a glance over the current view: in desktop mode a column on the right that a click on a game opens, in console mode a modal from a pad's X on a card; X again plays it.",
    parts: {
      art: "The hero or cover (a `game-art`).",
      close: "Closes it (a `button`).",
      platform: "Its platforms.",
      title: "The game's title.",
      actions: "Play, and View more beneath it.",
      play: "Play, or Stop while it runs (a `button`).",
      open: "View more: opens the game's page (a `button`).",
      notice: "Why it can't be played.",
      facts: "Playtime, last play, size and release.",
      playtime: "Time played.",
      "last-played": "When it was last played.",
      size: "Its size on disk.",
      released: "Its release date.",
      genres: "Its genres and tags.",
      description: "The game's description.",
    },
    states: {},
    interaction: [],
    contains: ["game-art", "status-badge", "button"],
  },
  "game-editor": {
    rendered: "GameEditor (app)",
    source: "app",
    description:
      "A game's editor, from its menu's Edit… or its page's Edit details: its details as one draft saved together, its artwork, and tabs for its launch actions and scripts. A dialog in desktop mode, the whole window in console mode.",
    parts: {
      title: "The editor's heading.",
      tabs: "The editor's sections: Details, Actions, Scripts.",
      tab: "One section; `data-selected` on the open one, `data-dirty` while it holds unsaved changes.",
      details: "The Details form.",
      field:
        "One field, with `data-field` naming it (`title`, `description`, `tags`, …), `data-manual` when its value is the user's and refreshes keep it, and `data-invalid` when the last save was refused for it.",
      label: "A field's name.",
      manual: "Marks a value the user set.",
      reset: "Puts a field or an artwork kind back to its automatic value (a `button`).",
      error: "Why a field wasn't saved, with the conflict choices.",
      "use-theirs": "Takes the value saved meanwhile (a `button`).",
      "keep-mine": "Keeps the user's value over the one saved meanwhile (a `button`).",
      list: "A list field's values and its add field.",
      chip: "One value of a list.",
      remove: "Removes a value from its list (a `button`).",
      footer: "Status, Cancel and Save.",
      status: "Saving, or what went wrong.",
      choice: "Who decides an artwork kind's image: automatic, chosen by the user, or none.",
      artwork: "The Artwork tab.",
      slot: "One kind of artwork (`data-kind`: `cover`, `hero`, `icon`; `data-choice`: `automatic`, `chosen`, `removed`; `data-drop-active` while an image is dragged over it).",
      preview: "The image that kind shows now.",
      candidate:
        "An image a source supplied, to choose (a `button`; `data-selected` on the one shown).",
      import: "Imports an image file (a `button`).",
      clear: "Shows no image of that kind (a `button`).",
      cancel: "Closes the editor, asking first when there are unsaved changes (a `button`).",
      save: "Saves the Details (a `button`).",
    },
    states: {},
    interaction: [],
    contains: ["button", "text-field", "checkbox", "slider"],
  },
  "manual-game": {
    rendered: "ManualGameDialog (app)",
    source: "app",
    description:
      "Adds a game by hand from Add game: one on this computer or behind a URI, started where it is, or a title-only record; games that may be the same offer to join or keep apart.",
    parts: {
      target:
        "What it starts: the path or URI field, Browse and the drop area (`data-drop-active` while a file is dragged over).",
      summary: "The program, arguments, folder or URI it will start.",
      duplicates: "Games that may be the same one.",
      duplicate: "One of them, and why.",
      link: "Adds the target to that game instead (a `button`).",
      error: "Why the target can't be added, or adding failed.",
      add: "Adds the game, or keeps it separate from the duplicates (a `button`).",
    },
    states: {
      "data-mode": ["target", "title"],
    },
    interaction: [],
    contains: ["button", "text-field"],
  },
  "selection-bar": {
    rendered: "SelectionBar (app)",
    source: "app",
    description:
      "The bar of a library or collection view in selection mode: how many games are chosen, choosing the loaded ones or all of them, clearing, the bulk editor and Done.",
    parts: {
      count: "How many games are chosen.",
      "select-loaded": "Chooses the games loaded so far (a `button`).",
      "select-all": "Chooses every game the view finds, with how many (a `button`).",
      clear: "Chooses none (a `button`).",
      edit: "Opens the bulk editor (a `button`).",
      refresh: "Refreshes the chosen games' details (a `button`).",
      done: "Leaves selection mode (a `button`).",
    },
    states: {},
    interaction: [],
    contains: ["button"],
  },
  "games-refresh-start": {
    rendered: "RefreshDialog (app)",
    source: "app",
    description:
      "Starts refreshing the chosen games' details and artwork from the built-in sources: missing details only, or the automatic ones fetched again.",
    parts: {
      start: "Starts the refresh (a `button`).",
    },
    states: {},
    interaction: [],
    contains: ["button", "segmented"],
  },
  "games-refresh": {
    rendered: "RefreshCard (app)",
    source: "app",
    description:
      "The refresh of chosen games' details in Activity: progress, counts, Cancel, and retrying failed games or continuing a cancelled refresh.",
    parts: {
      title: "What the refresh is doing.",
      mode: "Missing details only, or automatic details refreshed.",
      progress: "How many games are through.",
      counts: "Updated, unchanged, not found on IGDB, skipped and failed games.",
      cancel: "Stops after the current batch (a `button`).",
      retry: "Refreshes the games that failed again (a `button`).",
      continue: "Refreshes the games a cancelled refresh left (a `button`).",
    },
    states: {
      "data-status": ["running", "done", "cancelled"],
    },
    interaction: [],
    contains: ["button"],
  },
  "bulk-editor": {
    rendered: "BulkEditDialog (app)",
    source: "app",
    description:
      "One edit on the chosen games: the fields to change, a preview of what changes in which game, and what saved.",
    parts: {
      fields: "The fields, each changed only when ticked.",
      field: "One field (`data-field`; `data-enabled` when ticked).",
      list: "A list field's values and its add field.",
      chip: "One value of a list.",
      remove: "Removes a value from its list (a `button`).",
      results: "The preview or the result.",
      summary: "How many games change, already have it, can't take it or are gone.",
      change: "One game and what changes in it.",
      rejected: "A game that can't take the edit, and why.",
      footer: "Status, Back or Cancel, and Preview, Apply or Done.",
      status: "Working, or what went wrong.",
      cancel: "Back to the fields, or Cancel (a `button`).",
      apply: "Preview, Apply or Done (a `button`).",
    },
    states: {
      "data-step": ["edit", "preview", "done"],
    },
    interaction: [],
    contains: ["button", "checkbox", "segmented", "text-field", "slider"],
  },
  "game-editor-discard": {
    rendered: "GameEditor (app)",
    source: "app",
    description: "Asks before an editor with unsaved changes closes: Keep editing or Discard.",
    parts: {},
    states: {},
    interaction: [],
    contains: ["button"],
  },
  "game-actions": {
    rendered: "ActionsTab (app)",
    source: "app",
    description:
      "The game editor's Actions tab (#178): a release's launch actions, its own launch first, with the form for the one chosen and what Play starts.",
    parts: {
      default: "What Play starts: one of the play actions, or Ask each time.",
      actions: "The release's actions (a listbox).",
      action:
        "One action; `data-kind` is `play` or `utility`, `data-selected` on the one in the form.",
      form: "The chosen action's fields.",
      locked: "Says the release's own launch comes from its source and how to change it.",
      "target-kind":
        "What an emulator, web or plugin action starts, which the form doesn't change.",
      path: "A path with Browse…; `data-drop-active` while a dropped file is over it.",
      list: "A list of values, one field each: arguments, program names.",
      env: "The environment variables, a name and a value per row.",
      remove: "Removes one value or variable (a `button`).",
      proton:
        "Which Proton build a Windows program runs with: Automatic (the best installed) or one build.",
      tracking: "Which programs are the game, for a play action.",
      error: "Why the last save, order, delete or default change was refused.",
      delete: "Deletes the action; pressed once more to confirm (a `button`).",
    },
    states: {},
    interaction: [],
    contains: ["button", "select", "segmented", "text-field"],
  },
  "game-saves": {
    rendered: "SavesTab (app)",
    source: "app",
    description:
      "The game editor's Saves tab (#238): one release's save locations on this device, automatic or the user's own, and the manifest game it matches. Files and folders dropped on it are added.",
    parts: {
      match: "What the locations come from; `data-match` is `unknown`, `matched` or `ambiguous`.",
      mode: "Says whether the automatic locations or the user's own are in use.",
      locations: "The save locations (a list).",
      location: "One location; `data-kind` is `file`, `folder` or `registry`, with its file count.",
      path: "A location's path or registry key.",
      empty: "Shown when the release has no save locations.",
      warnings: "What couldn't be resolved on this device.",
      error: "Why the locations couldn't load or the last save was refused.",
    },
    states: { "data-choice": ["automatic", "chosen"] },
    interaction: [],
    contains: ["button", "select", "text-field"],
  },
  scripts: {
    rendered: "ScriptsPanel (app)",
    source: "app",
    description:
      "One scope's lifecycle scripts (#180), in Settings → Scripts, an emulator profile and the game editor: the scripts, the chosen one's form, its test and the order a launch runs every scope's scripts in.",
    parts: {
      items: "The scope's scripts (a listbox).",
      script:
        "One script; `data-enabled` is `false` for one that's off, `data-selected` on the one in the form.",
      form: "The chosen script's fields.",
      path: "A path with Browse… (the program, the working folder); `data-drop-active` while a dropped file is over it.",
      list: "A program's arguments, one field each.",
      remove: "Removes one argument (a `button`).",
      error: "Why the last save, order or delete was refused.",
      delete: "Deletes the script; pressed once more to confirm (a `button`).",
      test: "Test and Cancel test, and the last test's result.",
      result:
        "A test's outcome; `data-status` is how it ended (`succeeded`, `failed`, `timed_out`, `cancelled`, `not_started`).",
      output: "A test's standard output or errors.",
      context: "The values the test saw.",
      order: "The order a launch runs scripts in, every scope's.",
      step: "One step; `data-inherited` when it comes from another scope, `data-enabled` `false` when it's off.",
    },
    states: {},
    interaction: [],
    contains: ["button", "select", "segmented", "switch", "text-field"],
  },
  "play-choice": {
    rendered: "PlayChoice (app)",
    source: "app",
    description:
      "On a game's page, under Play: which of the release's play actions Play starts, and its tools (#178).",
    parts: {
      tools: "The release's tools, one button each.",
    },
    states: {},
    interaction: [],
    contains: ["button", "select"],
  },
  "tool-failed": {
    rendered: "ToolRequest (app)",
    source: "app",
    description: "Says why a game's tool didn't start.",
    parts: {},
    states: {},
    interaction: [],
    contains: ["button"],
  },
  "screenshot-viewer": {
    rendered: "ScreenshotViewer (app)",
    source: "app",
    description:
      "A game's screenshots one at a time, filling the window, opened from its page; arrows, a swipe, a pad's left/right or LB/RB step through them.",
    parts: {
      title: "The game's title.",
      counter: "Which screenshot of how many.",
      close: "Closes the gallery.",
      stage: "The area the screenshot fills; a swipe on it steps.",
      image: "The screenshot, full size.",
      prev: "Steps to the previous screenshot.",
      next: "Steps to the next screenshot.",
      strip: "Every screenshot, small, to jump to one.",
      thumb: "One screenshot in the strip; `data-selected` on the one shown.",
    },
    states: {},
    interaction: [],
  },
  videos: {
    rendered: "Videos (app)",
    source: "app",
    description:
      "A game's videos (IGDB's trailers) in a row on its page and its Store page; each plays in the video window.",
    parts: {
      items: "The row of videos.",
      video: "One video's button: its thumbnail and name.",
      thumbnail: "The video's thumbnail.",
      play: "The play mark over the thumbnail.",
      name: "The video's name.",
    },
    states: {},
    interaction: ["data-hovered", "data-focus-visible", "data-pressed"],
  },
  osk: {
    rendered: "OnScreenKeyboard (app)",
    source: "app",
    description:
      "The on-screen keyboard a pad types with; it types into the focused field, which keeps focus.",
    parts: {
      preview: "The field's text as it is typed.",
      row: "A row of keys.",
      key: "A key; `data-key` is its character or what it does (`shift`, `layer`, `backspace`, `space`, `left`, `right`, `done`), `data-focused` is the pad's place, `data-selected` an active shift.",
    },
    states: { "data-layer": ["letters", "symbols"], "data-shift": ["off", "once", "lock"] },
    interaction: [],
  },
  "type-search": {
    rendered: "TypeSearch (app)",
    source: "app",
    description:
      "Search from a keyboard, RetroArch style: a centred overlay with the text being typed and how many games match, opened by typing anywhere outside a text field or by `/`.",
    parts: {
      label: "The “Search” label.",
      input: "The `<input>` holding the typed text.",
      count: "How many games match.",
      hints: "The keys for the results, cancelling and deleting.",
    },
    states: {},
    interaction: [],
  },
  "folder-browser": {
    rendered: "FolderBrowser (app)",
    source: "app",
    description:
      "Console mode's file and folder picker, in place of the native dialogs a controller can't drive. A opens a folder or picks a file, B cancels, LB/RB go to the previous and next place, LT/RT page through the folder, Start picks the folder shown. `data-kind` is what it picks.",
    parts: {
      title: "What is being picked.",
      location: "The folder shown, as a path.",
      places: "Home, the filesystem root and mounted drives.",
      place: 'One place; the one the folder is in has `data-status="active"`.',
      error: "Why a folder couldn't be opened.",
      entries: "The folder's contents, scrolling.",
      up: "Goes up one folder.",
      entry:
        "A folder or file; `data-kind` is `folder` or `file`. Files that can't be picked carry `data-disabled`.",
      icon: "An entry's icon.",
      name: "An entry's name.",
      empty: "Shown in a folder with nothing in it.",
      actions: "Cancel, and in folder mode Select this folder.",
      select: "Picks the folder shown (a `button`).",
    },
    states: { "data-kind": ["folder", "file"] },
    interaction: [],
    contains: ["button"],
  },
  "store-links": {
    rendered: "StoreLinks (app)",
    source: "app",
    description: "Links to the stores that sell a game.",
    parts: { "store-link": "One store; `data-store` names it." },
    states: {},
    interaction: [],
  },
  "peer-link": {
    rendered: "LinkBadge (app)",
    source: "app",
    description:
      "How another device is reached (#225, #231): an icon and a word. `data-status` is `lan` (on this network), `direct` (over the internet), `relay` (through a relay) or `offline`.",
    parts: {},
    states: { "data-status": ["lan", "direct", "relay", "offline"] },
    interaction: [],
  },
  "paired-device": {
    rendered: "DeviceRow in Settings → Devices (app)",
    source: "app",
    description:
      "A device paired with this one: its icon, name, how it is reached, its last sync, Sync now and (for an admin) Forget. `data-status` is how it is reached.",
    parts: {
      sync: "Its OS and the last sync, or that it is syncing now.",
      "sync-error": "Shown when the last sync didn't reach it.",
    },
    states: { "data-status": ["lan", "direct", "relay", "offline"] },
    interaction: [],
    contains: ["peer-link", "button"],
  },
  transfer: {
    rendered: "CopiesAndShares in Settings → Devices (app)",
    source: "app",
    description:
      "A game copied between two of the user's devices (#233): its title, the other device, how it is reached and its progress. `data-status` is `running`, `waitingDirect` (game files wait for a direct path), `done` or `failed`.",
    parts: {},
    states: { "data-status": ["running", "waitingDirect", "done", "failed"] },
    interaction: [],
    contains: ["peer-link"],
  },
  share: {
    rendered: "SharesList in Settings → Devices → Copies and shares (app)",
    source: "app",
    description:
      "A game this device shared once with someone else (#246, #247): its title and size, Copy link and Stop sharing while it runs (Forget after), what it is doing and who received it. `data-status` is `preparing` (its files are being read; the link works already), `ready`, `failed`, `stopped` or `expired`.",
    parts: {
      state:
        "What the share is doing: reading its files, until when the link works, or that it stopped.",
      recipients: "Who opened the link, each with how far they got.",
    },
    states: { "data-status": ["preparing", "ready", "failed", "stopped", "expired"] },
    interaction: [],
    contains: ["share-recipient"],
  },
  "share-recipient": {
    rendered: "ShareRecipients (app)",
    source: "app",
    description:
      "Someone who opened a share's link (#247): their device's name, whether they help others receive it, and a bar of how far they got. `data-status` is `receiving`, `away` (not connected now) or `done`.",
    parts: {},
    states: { "data-status": ["receiving", "away", "done"] },
    interaction: [],
  },
  "received-share": {
    rendered: "SharesList in Settings → Devices → Copies and shares (app)",
    source: "app",
    description:
      "A game someone shared with this device (#249): its title and sharer, and the Help others receive it switch. `data-status` is `receiving`, `serving` (it helps the share's other recipients) or `done`.",
    parts: {},
    states: { "data-status": ["receiving", "serving", "done"] },
    interaction: [],
  },
  "share-game": {
    rendered: "ShareGameDialog (app)",
    source: "app",
    description:
      "Share with someone… (#247): how long the link works, then the link with Copy (and, in console mode, a QR code), what the share is doing, who received it, and Stop sharing.",
    parts: {
      link: "The share link, selectable.",
      qr: "The link as a QR code, in console mode.",
      state: "What the share is doing.",
      recipients: "Who opened the link.",
    },
    states: {},
    interaction: [],
    contains: ["share-recipient"],
  },
  "receive-share": {
    rendered: "ReceiveShareDialog (app)",
    source: "app",
    description:
      "Add game → Receive a shared game… (#247): the link, what its sharer says it shares, Help others receive it, and Receive.",
    parts: {},
    states: {},
    interaction: [],
    contains: ["share-info"],
  },
  "share-info": {
    rendered: "ShareInfoDetails (app)",
    source: "app",
    description:
      "What a share link shares, as its sharer says (#247, #248): who, what, its size and build, until when the link works; for a save, web game or ROM, where it goes here; for a collection, which of its games this device has. `data-status` is `direct`, or `relayOnly` when only a relay reaches the sharer and files wait for a direct connection.",
    parts: {
      where: "Where a save, web game or ROM goes on this device.",
      games: "A shared collection's games, each with whether it's here (`data-status`).",
    },
    states: { "data-status": ["direct", "relayOnly"] },
    interaction: [],
  },
  "pair-device": {
    rendered: "PairingHost (app)",
    source: "app",
    description:
      "Pairing over any view (#226, #231): the code both devices show, for a pairing this device or the other one started; or a `caskade://pair` link opened from outside, waiting for an admin.",
    parts: {
      state: "A code pairing; `data-status` is `waiting`, `paired`, `cancelled` or `failed`.",
      code: "The six-digit code, split in two; larger in console mode.",
      link: "An opened pairing link; `data-status` is `valid` or `expired`.",
    },
    states: {},
    interaction: [],
    contains: ["button"],
  },
  "account-picker": {
    rendered: "AccountPicker (app)",
    source: "app",
    description:
      "Who is playing: the accounts to sign in as, at start and after Switch account, and the PIN pad for a locked one. `data-step` is `accounts` or `pin`.",
    parts: {
      titlebar: "The bar along the top: it drags the window, and holds the window controls.",
      heading: "The title.",
      accounts: "The account tiles.",
      account:
        'An account\'s tile; `data-status="locked"` when a PIN locks it. The last account to sign in has `data-focus-default`.',
      avatar: "An account's picture (an `avatar`).",
      name: "An account's name.",
      lock: "The lock on a PIN-locked account's tile.",
      back: "Back from the PIN pad to the accounts.",
    },
    states: { "data-step": ["accounts", "pin"] },
    interaction: [],
    contains: ["avatar", "pin-pad", "window-controls", "hint-bar"],
  },
  "pin-pad": {
    rendered: "PinPad (app)",
    source: "app",
    description:
      "PIN entry: a field for keys and a number pad for a controller (A types a digit, B deletes, Start confirms). A wrong PIN shakes the field and clears it; after too many, `locked` counts the wait down.",
    parts: {
      title: "What the PIN is for.",
      field: "The PIN, as dots.",
      keys: "The number pad.",
      key: "A digit.",
      delete: "Deletes the last digit.",
      submit: "Confirms the PIN.",
      message: "Why the PIN was refused, or how long to wait.",
    },
    states: { "data-status": ["ready", "wrong", "locked"] },
    interaction: [],
  },
  "account-menu": {
    rendered: "AccountMenu (app)",
    source: "app",
    description:
      "The head of the menu the topbar's avatar opens: who is signed in. Its actions are the `menu`'s items.",
    parts: {
      avatar: "The account's picture (an `avatar`).",
      name: "The account's name.",
      role: "Admin or Standard.",
    },
    states: {},
    interaction: [],
    contains: ["avatar"],
  },
  "account-list": {
    rendered: "AccountsSettings (app)",
    source: "app",
    description: "Settings → Accounts, for admins: every account, with Edit and Delete.",
    parts: {
      account: "One account's row.",
      avatar: "Its picture (an `avatar`).",
      name: "Its name; the signed-in account's says so.",
      details: "Its role, whether a PIN locks it, and what it may do.",
      edit: "Opens the `account-editor` (a `button`).",
      delete: "Asks before deleting the account and its collections (a `button`).",
    },
    states: {},
    interaction: [],
    contains: ["avatar", "button"],
  },
  "account-editor": {
    rendered: "AccountEditor (app)",
    source: "app",
    description:
      "The dialog that adds or changes an account: its name, avatar, role and PIN, and for a standard account what it may change and its age limit.",
    parts: {
      avatars: "The avatar choice (an `avatar-picker`) and its label.",
      access:
        "A standard account's permissions: Install games, one switch per Settings section, the maximum age rating and unrated games.",
    },
    states: {},
    interaction: [],
    contains: [
      "text-field",
      "avatar-picker",
      "setting-row",
      "segmented",
      "switch",
      "select",
      "button",
    ],
  },
  "account-profile": {
    rendered: "ProfileSettings (app)",
    source: "app",
    description: "Settings → My profile: the signed-in account's own name, avatar and PIN.",
    parts: {
      avatars: "The avatar choice (an `avatar-picker`) and its label.",
      pin: "The PIN's row: set, change or remove it, after the current PIN.",
    },
    states: {},
    interaction: [],
    contains: ["text-field", "avatar-picker", "setting-row", "button"],
  },
  "addon-catalogue": {
    rendered: "AddonCatalogue (app)",
    source: "app",
    description:
      "Settings → Plugins' catalogue of curated plugins and themes: what each means here, searchable and by kind, from the last fetch when offline.",
    parts: {
      refresh: "Check for updates: fetches the catalogue again (a `button`).",
      status: "When the catalogue was fetched, offline or why the last check failed.",
      updates: "How many updates are waiting.",
      "update-preference": "Whether to check for updates when Settings opens (a `switch`).",
      item: "One add-on (`data-kind`: `plugin` or `theme`).",
      state:
        "What the add-on means here (`data-state`: `available`, `installed`, `updateAvailable` or `incompatible`).",
      details: "Opens the add-on's details (a `button`).",
    },
    states: {},
    interaction: [],
    contains: ["button", "switch", "text-field", "segmented"],
  },
  "addon-details": {
    rendered: "AddonDetails (app)",
    source: "app",
    description:
      "A catalogue add-on's details: screenshots, description as text, compatibility, the release an install gets, and installing, updating, applying and removing it.",
    parts: {
      screenshots: "The catalogue's screenshots.",
      screenshot: "One screenshot, a thumbnail the app re-encoded.",
      "screenshot-missing": "In place of a screenshot that couldn't load.",
      description: "The description, always shown as text.",
      reasons: "Why the add-on doesn't fit here, or what it lacks.",
      release: "The release an install gets: version, date, OSes, modes and permissions.",
      state:
        "What the add-on means here (`data-state`: `available`, `installed`, `updateAvailable` or `incompatible`).",
      progress: "Downloading, checking, installing or starting.",
      outcome:
        "What the last install did (`data-outcome`: `installed` or `failed`), or a release refused before.",
      "remove-confirm":
        "Asks before removing; for a plugin, whether to keep its settings and data.",
      "remove-confirmed": "Removes it (a `button`).",
      actions: "The add-on's buttons.",
      remove: "Uninstall… (a `button`).",
      skip: "Skips this update, or offers it again (a `button`).",
      apply: "Makes an installed theme the one in use (a `button`).",
      install: "Install, Update or Try again (a `button`).",
    },
    states: {
      "data-kind": ["plugin", "theme"],
      "data-state": ["available", "installed", "updateAvailable", "incompatible"],
    },
    interaction: [],
    contains: ["button", "checkbox"],
  },
  "plugin-ui": {
    rendered: "PluginTree (app)",
    source: "app",
    description:
      "What a plugin draws in a slot or view (plugin API 0.2): Caskade's components as the plugin's backend asked for them.",
    parts: {
      text: "Plain text.",
      heading: "A heading (`h2`, or `h3` at level 3).",
      paragraph: "A paragraph.",
      note: "Secondary text, muted.",
      alert: "Something that went wrong, announced.",
      stack: "Children in a column or a row (`data-direction`).",
      section: "A labelled part that pad focus moves between.",
      stats: "Labelled values (a description list).",
      tile: "One large value with what it is.",
      value: "A tile's value.",
      label: "A tile's label.",
      bars: "A bar chart, with each value as text.",
      bar: "One bar.",
      table: "A table.",
      list: "A list.",
      badge: "A short status label (`data-tone`).",
      progress: "A progress bar.",
      button: "A button that tells the plugin (a `button`).",
      segmented: "One of a few options that tells the plugin (a `segmented`).",
    },
    states: {},
    interaction: [],
    contains: ["button", "segmented"],
  },
  "plugin-error": {
    rendered: "PluginBoundary (app)",
    source: "app",
    description:
      "In place of a plugin's slot or view that failed to draw; it holds focus, so a pad can move on.",
    parts: {},
    states: {},
    interaction: [],
    contains: [],
  },
  achievements: {
    rendered: "GameAchievements (app)",
    source: "app",
    description:
      "A game page's achievements: one store account's set at a time, its counts and how its last fetch went, and every achievement with its state, progress, unlock time, rarity and points where the service gives them. Hidden achievements stay concealed until unlocked or revealed.",
    parts: {
      refresh: "Refresh achievements: fetches the game's sets now (a `button`).",
      cancel: "Stops a refresh under way (a `button`).",
      sets: "Which store account's set is shown, when there are several (a `segmented`).",
      account: "The set's service and account, and when it was last fetched.",
      stale: "The last refresh failed: what is shown is the last answer, and why.",
      wait: "The service asked Caskade to wait, and until when.",
      notice:
        "Not fetched yet, no achievements, not offered, private, or signed out, with what to do.",
      reconnect: "Opens Settings → Store accounts to sign in again (a `button`).",
      summary: "Unlocked of total, hardcore unlocks and points, with a meter.",
      filter: "All, unlocked or locked (a `segmented`).",
      "reveal-all": "Shows every hidden achievement (a `switch`).",
      list: "The achievements, in the service's order.",
      achievement:
        "One achievement (`data-state`: `unlocked` or `locked`; `data-concealed` while a hidden one is concealed; `data-hardcore` when unlocked in hardcore).",
      icon: "Its icon from the service, or a drawn trophy, lock or concealed mark.",
      details: "Unlock time, hardcore, rarity and points.",
      hardcore: "Its hardcore unlock, or that it was unlocked in softcore only.",
      reveal: "Shows a hidden achievement's name and description (a `button`).",
    },
    states: {
      "data-state": [
        "loading",
        "error",
        "not_fetched",
        "ready",
        "no_achievements",
        "unsupported",
        "private",
        "sign_in_needed",
      ],
    },
    interaction: [],
    contains: ["button", "segmented", "switch"],
  },
  "achievement-summary": {
    rendered: "AchievementSummary (app)",
    source: "app",
    description:
      "A game's achievement counts in its hero, across its sets; pressing it moves to the achievements.",
    parts: {},
    states: {},
    interaction: [],
  },
  romm: {
    rendered: "RommPanel (app)",
    source: "app",
    description:
      "Settings → RomM servers: the server list, the selected server's editor, downloads and the cache.",
    parts: {},
    states: {},
    interaction: [],
    contains: ["settings-section", "romm-servers"],
    container: true,
  },
  "romm-servers": {
    rendered: "RommPanel (app)",
    source: "app",
    description:
      "The RomM server list: each server's state, games, last sync and its action (Sync now, or Sign in when this device can't sign in).",
    parts: {
      "romm-server":
        "A server's row (`data-status`: `untested`, `connected`, `offline`, `error` or `sign_in_needed`; `data-selected` while its editor is open).",
      "sign-in":
        "Sign in or Enter API token on a server this device can't sign in to: opens its editor at the password or token (a `button`).",
      "sign-in-reason":
        "Why a server needs signing in on this device: sign-ins stay on each device, or the server refused the saved one.",
    },
    states: {},
    interaction: [],
    contains: ["button", "status-dot"],
  },
} as const satisfies Record<string, AnatomyEntry>;

export type AnatomyName = keyof typeof anatomy;

/** Attributes a theme may select on besides each root's own states and
 * interaction: app-wide modes on the document root, and states of parts. */
export const contractAttributes = {
  root: [
    "data-mode",
    "data-color-scheme",
    "data-effects",
    "data-motion",
    "data-density",
    "data-paint",
  ],
  parts: [
    "data-status",
    "data-store",
    "data-selected",
    "data-focused",
    "data-focus-within",
    "data-focus-visible",
    "data-hovered",
    "data-pressed",
    "data-dragging",
    "data-disabled",
  ],
} as const;

export const anatomyNames = Object.keys(anatomy) as AnatomyName[];

/** The catalogued roots inside `container`, itself included. */
export function anatomyRoots(container: ParentNode): Element[] {
  const roots = [...container.querySelectorAll("[data-cs]")];
  if (container instanceof Element && container.hasAttribute("data-cs")) roots.unshift(container);
  return roots.filter((root) => (root.getAttribute("data-cs") ?? "") in anatomy);
}

/** The parts a root owns: `data-part` elements whose nearest enclosing root
 * is this one. A nested component's root can be a part of its parent too. */
export function ownedParts(root: Element): Element[] {
  return [...root.querySelectorAll("[data-part]")].filter(
    (element) => element.parentElement?.closest("[data-cs]") === root,
  );
}

/** Whether `element` sits inside one of `root`'s slot parts. */
function inSlot(root: Element, element: Element, slots: readonly string[]): boolean {
  for (let node = element.parentElement; node && node !== root; node = node.parentElement)
    if (slots.includes(node.getAttribute("data-part") ?? "")) return true;
  return false;
}

/** What is wrong with a rendered root against its catalogue entry: unknown
 * parts, state values the catalogue doesn't list. Empty when it conforms. */
export function checkAnatomy(root: Element): string[] {
  const name = root.getAttribute("data-cs") ?? "";
  if (!(name in anatomy)) return [`"${name}" is not in the anatomy catalogue`];
  const entry: AnatomyEntry = anatomy[name as AnatomyName];
  const problems: string[] = [];
  for (const part of ownedParts(root)) {
    const value = part.getAttribute("data-part") ?? "";
    if (value in entry.parts || entry.container || inSlot(root, part, entry.slots ?? [])) continue;
    problems.push(`${name}: undocumented part "${value}"`);
  }
  for (const [attribute, values] of Object.entries(entry.states)) {
    const value = root.getAttribute(attribute);
    if (value !== null && !values.includes(value))
      problems.push(`${name}: ${attribute}="${value}" is not one of ${values.join(", ")}`);
  }
  return problems;
}
