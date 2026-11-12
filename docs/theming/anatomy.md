<!-- Generated from the native UI's components (apps/native/src/anatomy.rs) and, for roots only the webview renders yet, packages/ui/src/anatomy.ts: CASKADE_WRITE_ANATOMY=1 cargo test -p caskade-native anatomy. Do not edit. -->

# Anatomy catalogue

The selectors a theme or plugin may target (see [README](README.md)). Each
component renders `data-cs="<name>"` on its root and `data-part="<part>"` on
its parts. A part belongs to the nearest enclosing root, so
`[data-cs="game-card"] [data-part="title"]` does not match a title inside a
nested component. A nested component's root can also be a part of its
parent: a card's art is `[data-cs="game-art"][data-part="art"]`.

Domain state is an attribute on the root, such as `data-status` or
`data-variant`. Interaction state comes from React Aria
(`data-hovered`, `data-pressed`, `data-focus-visible`, `data-selected`,
`data-disabled`, …). Class names are not API and change without notice.

Parts and states marked (app only) are rendered only by the app's own
variant of the component, such as the library's card.

## `@caskade/ui` components

- [`button`](#button): A push button.
- [`checkbox`](#checkbox): A checkbox with its label.
- [`segmented`](#segmented): One choice from a few options shown side by side.
- [`select`](#select): A dropdown choice; its list opens in a `select-popover`.
- [`select-popover`](#select-popover): The open list of a `select`, rendered in a portal.
- [`menu`](#menu): A menu of actions, rendered in a portal: a button's dropdown, or a right click's menu at the pointer.
- [`slider`](#slider): A value on a track.
- [`switch`](#switch): An on/off toggle.
- [`text-field`](#text-field): A single-line text input.
- [`settings-section`](#settings-section): A titled group of settings.
- [`setting-row`](#setting-row): One setting: its label on the left, its controls on the right.
- [`status-dot`](#status-dot): A small coloured dot and label, e.g. Detected / Not found.
- [`pill`](#pill): A bordered chip with an icon, e.g. Off by default.
- [`game-art`](#game-art): A game's cover, or a blank placeholder when it has none.
- [`status-badge`](#status-badge): A game's install status.
- [`game-card`](#game-card): A game tile in a grid.
- [`hero`](#hero): The large featured-game banner.
- [`shelf`](#shelf): A titled row of cards.
- [`avatar`](#avatar): An account's picture: one of the shipped avatars, or the name's initials when its id is unknown or the image can't load.
- [`error-notice`](#error-notice): A failure, shown the same way everywhere: what failed and why, what to do, and the technical details behind a toggle.
- [`avatar-picker`](#avatar-picker): A grid of the shipped avatars to choose one from (a radio group).

## App shell

- [`app-shell`](#app-shell): The window: sidebar, topbar, current view and persistent status bar.
- [`status-bar`](#status-bar): Persistent background activity, application version and an available Caskade update. IGDB and Hasheous are credited in Settings → About (`about`).
- [`backdrop`](#backdrop): The decorative layer behind everything.
- [`onboarding`](#onboarding): First-run setup, shown in place of the app shell: a welcome, the questions (`language`, `folder`, `sources`, `mode`, and `account`, the admin's name, avatar and PIN), then the work they start (`setup`). Things arriving use `animate-rise`; the running task's sweep pauses with `--cs-motion-ambient`.
- [`sidebar`](#sidebar): The navigation column.
- [`topbar`](#topbar): The bar above the view: search, quick actions and the window controls. It is the window's title bar: dragging it moves the window.
- [`window-controls`](#window-controls): Minimize, maximize and close at the end of the topbar (Windows and Linux; macOS keeps its traffic lights). Each button carries `data-hovered` and `data-pressed`.
- [`view-error`](#view-error): Shown in place of a view that failed to render, with the shell around it; or in place of the whole window when the shell itself failed, with the window controls.
- [`console-shell`](#console-shell): Console mode's window (`[data-mode="console"]` on the root): the console sidebar, top bar and view, and the button-hint bar. `data-cursor` is `hidden` while the pointer rests.
- [`console-sidebar`](#console-sidebar): Console mode's navigation: big targets.
- [`console-topbar`](#console-topbar): Console mode's top bar: search and activity.
- [`notifications`](#notifications): Activity's notification feed (#198): game additions in hourly batches, failed jobs, updates on offer and plugins turned off, newest change first.
- [`notification`](#notification): One item of the notification feed. `data-category` is what it is about; `data-resolved` is set once its problem has cleared.
- [`hint-bar`](#hint-bar): What each controller button does where focus is. `data-glyphs` is the controller family in hand: `xbox`, `playstation`, `nintendo`, `generic`, or `keyboard`.
- [`game-curtain`](#game-curtain): Console mode's black screen behind a running game: it fades in when a game starts, so the game's window takes over without Caskade minimizing, and fades out when the last game exits. Always present in console mode; `data-state` is `covered` while it is down.
- [`shelf-row`](#shelf-row): A row of game cards that pages with LB/RB (Home's shelves).
- [`search`](#search): The game search; suggestions open in a `search-popover`.
- [`search-popover`](#search-popover): The search suggestions, rendered in a portal.
- [`view`](#view): The scrolling area that shows the current page.
- [`settings-nav`](#settings-nav): Settings' sections: a vertical tab list beside the open section's panel, in desktop and console modes, grouped under headings, with the settings search above it. It stays in view while the panel scrolls, and scrolls on its own when the sections don't fit.
- [`about`](#about): Settings → About: the IGDB and Hasheous credits first, side by side, then the app, its version and build, project links, and the links into `legal`.
- [`legal`](#legal): Settings → About → Legal: licences, open-source packages, terms and privacy policy, bundled for offline reading. One section shows at a time, at `/settings?tab=about&section=<id>`.
- [`system-stats`](#system-stats): Settings → Statistics: this machine's CPU, GPUs, memory, network and storage capacity, live, as the same cards in desktop and console modes. `data-status` says whether readings are coming in.
- [`panel`](#panel): A framed panel inside a view.
- [`collection-card`](#collection-card): A collection tile.
- [`platform-card`](#platform-card): A platform in the library's breakdown of emulated games; opens its games.
- [`game-row`](#game-row): A game in the library's compact table (the list layout).
- [`alphabet-bar`](#alphabet-bar): The letters beside a long library sorted by title; each jumps to its games, and a drag jumps letter by letter.
- [`quick-look`](#quick-look): A game at a glance over the current view: in desktop mode a column on the right that a click on a game opens, in console mode a modal from a pad's X on a card; X again plays it.
- [`game-editor`](#game-editor): A game's editor, from its menu's Edit… or its page's Edit details: its details as one draft saved together, its artwork, and tabs for its launch actions and scripts. A dialog in desktop mode, the whole window in console mode.
- [`manual-game`](#manual-game): Adds a game by hand from Add game: one on this computer or behind a URI, started where it is, or a title-only record; games that may be the same offer to join or keep apart.
- [`selection-bar`](#selection-bar): The bar of a library or collection view in selection mode: how many games are chosen, choosing the loaded ones or all of them, clearing, the bulk editor and Done.
- [`games-refresh-start`](#games-refresh-start): Starts refreshing the chosen games' details and artwork from the built-in sources: missing details only, or the automatic ones fetched again.
- [`games-refresh`](#games-refresh): The refresh of chosen games' details in Activity: progress, counts, Cancel, and retrying failed games or continuing a cancelled refresh.
- [`bulk-editor`](#bulk-editor): One edit on the chosen games: the fields to change, a preview of what changes in which game, and what saved.
- [`game-editor-discard`](#game-editor-discard): Asks before an editor with unsaved changes closes: Keep editing or Discard.
- [`game-actions`](#game-actions): The game editor's Actions tab (#178): a release's launch actions, its own launch first, with the form for the one chosen and what Play starts.
- [`game-saves`](#game-saves): The game editor's Saves tab (#238): one release's save locations on this device, automatic or the user's own, and the manifest game it matches. Files and folders dropped on it are added.
- [`scripts`](#scripts): One scope's lifecycle scripts (#180), in Settings → Scripts, an emulator profile and the game editor: the scripts, the chosen one's form, its test and the order a launch runs every scope's scripts in.
- [`play-choice`](#play-choice): On a game's page, under Play: which of the release's play actions Play starts, and its tools (#178).
- [`tool-failed`](#tool-failed): Says why a game's tool didn't start.
- [`screenshot-viewer`](#screenshot-viewer): A game's screenshots one at a time, filling the window, opened from its page; arrows, a swipe, a pad's left/right or LB/RB step through them.
- [`videos`](#videos): A game's videos (IGDB's trailers) in a row on its page and its Store page; each plays in the video window.
- [`osk`](#osk): The on-screen keyboard a pad types with; it types into the focused field, which keeps focus.
- [`type-search`](#type-search): Search from a keyboard, RetroArch style: a centred overlay with the text being typed and how many games match, opened by typing anywhere outside a text field or by `/`.
- [`folder-browser`](#folder-browser): Console mode's file and folder picker, in place of the native dialogs a controller can't drive. A opens a folder or picks a file, B cancels, LB/RB go to the previous and next place, LT/RT page through the folder, Start picks the folder shown. `data-kind` is what it picks.
- [`store-links`](#store-links): Links to the stores that sell a game.
- [`peer-link`](#peer-link): How another device is reached (#225, #231): an icon and a word. `data-status` is `lan` (on this network), `direct` (over the internet), `relay` (through a relay) or `offline`.
- [`paired-device`](#paired-device): A device paired with this one: its icon, name, how it is reached, its last sync, Sync now and (for an admin) Forget. `data-status` is how it is reached.
- [`transfer`](#transfer): A game copied between two of the user's devices (#233): its title, the other device, how it is reached and its progress. `data-status` is `running`, `waitingDirect` (game files wait for a direct path), `done` or `failed`.
- [`share`](#share): A game this device shared once with someone else (#246, #247): its title and size, Copy link and Stop sharing while it runs (Forget after), what it is doing and who received it. `data-status` is `preparing` (its files are being read; the link works already), `ready`, `failed`, `stopped` or `expired`.
- [`share-recipient`](#share-recipient): Someone who opened a share's link (#247): their device's name, whether they help others receive it, and a bar of how far they got. `data-status` is `receiving`, `away` (not connected now) or `done`.
- [`received-share`](#received-share): A game someone shared with this device (#249): its title and sharer, and the Help others receive it switch. `data-status` is `receiving`, `serving` (it helps the share's other recipients) or `done`.
- [`share-game`](#share-game): Share with someone… (#247): how long the link works, then the link with Copy (and, in console mode, a QR code), what the share is doing, who received it, and Stop sharing.
- [`receive-share`](#receive-share): Add game → Receive a shared game… (#247): the link, what its sharer says it shares, Help others receive it, and Receive.
- [`share-info`](#share-info): What a share link shares, as its sharer says (#247, #248): who, what, its size and build, until when the link works; for a save, web game or ROM, where it goes here; for a collection, which of its games this device has. `data-status` is `direct`, or `relayOnly` when only a relay reaches the sharer and files wait for a direct connection.
- [`pair-device`](#pair-device): Pairing over any view (#226, #231): the code both devices show, for a pairing this device or the other one started; or a `caskade://pair` link opened from outside, waiting for an admin.
- [`account-picker`](#account-picker): Who is playing: the accounts to sign in as, at start and after Switch account, and the PIN pad for a locked one. `data-step` is `accounts` or `pin`.
- [`pin-pad`](#pin-pad): PIN entry: a field for keys and a number pad for a controller (A types a digit, B deletes, Start confirms). A wrong PIN shakes the field and clears it; after too many, `locked` counts the wait down.
- [`account-menu`](#account-menu): The head of the menu the topbar's avatar opens: who is signed in. Its actions are the `menu`'s items.
- [`account-list`](#account-list): Settings → Accounts, for admins: every account, with Edit and Delete.
- [`account-editor`](#account-editor): The dialog that adds or changes an account: its name, avatar, role and PIN, and for a standard account what it may change and its age limit.
- [`account-profile`](#account-profile): Settings → My profile: the signed-in account's own name, avatar and PIN.
- [`addon-catalogue`](#addon-catalogue): Settings → Plugins' catalogue of curated plugins and themes: what each means here, searchable and by kind, from the last fetch when offline.
- [`addon-details`](#addon-details): A catalogue add-on's details: screenshots, description as text, compatibility, the release an install gets, and installing, updating, applying and removing it.
- [`plugin-ui`](#plugin-ui): What a plugin draws in a slot or view (plugin API 0.2): Caskade's components as the plugin's backend asked for them.
- [`plugin-error`](#plugin-error): In place of a plugin's slot or view that failed to draw; it holds focus, so a pad can move on.
- [`achievements`](#achievements): A game page's achievements: one store account's set at a time, its counts and how its last fetch went, and every achievement with its state, progress, unlock time, rarity and points where the service gives them. Hidden achievements stay concealed until unlocked or revealed.
- [`achievement-summary`](#achievement-summary): A game's achievement counts in its hero, across its sets; pressing it moves to the achievements.
- [`romm`](#romm): Settings → RomM servers: the server list, the selected server's editor, downloads and the cache.
- [`romm-servers`](#romm-servers): The RomM server list: each server's state, games, last sync and its action (Sync now, or Sign in when this device can't sign in).

## Website

- [`website`](#website): The public landing page, sharing the app's tokens and branding.

## `website`

The public landing page, sharing the app's tokens and branding. Rendered by apps/website/index.html (static GitHub Pages site).

Root: `[data-cs="website"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `skip` | `[data-cs="website"] [data-part="skip"]` | Keyboard skip link to the main content. |
| `header` | `[data-cs="website"] [data-part="header"]` | Brand, primary navigation, language dropdown and download link. |
| `main-nav` | `[data-cs="website"] [data-part="main-nav"]` | The main page navigation links. |
| `header-actions` | `[data-cs="website"] [data-part="header-actions"]` | The language dropdown and download link. |
| `language-trigger` | `[data-cs="website"] [data-part="language-trigger"]` | Native popover button with the current language flag and name. |
| `language-options` | `[data-cs="website"] [data-part="language-options"]` | Language links with flags; aria-current marks the current page. |
| `brand` | `[data-cs="website"] [data-part="brand"]` | Caskade's mark and name. |
| `button` | `[data-cs="website"] [data-part="button"]` | A link styled as a button; data-variant is primary or small when set. |
| `hero` | `[data-cs="website"] [data-part="hero"]` | Introduction and desktop and console screenshots. |
| `hero-copy` | `[data-cs="website"] [data-part="hero-copy"]` | The main heading, introduction and actions. |
| `eyebrow` | `[data-cs="website"] [data-part="eyebrow"]` | A short section introduction. |
| `status-dot` | `[data-cs="website"] [data-part="status-dot"]` | A decorative accent beside the introduction. |
| `intro` | `[data-cs="website"] [data-part="intro"]` | The landing page introduction. |
| `actions` | `[data-cs="website"] [data-part="actions"]` | Download and source link groups, including platform installer choices. |
| `text-link` | `[data-cs="website"] [data-part="text-link"]` | A text link to a project resource. |
| `hero-note` | `[data-cs="website"] [data-part="hero-note"]` | Free software and privacy summary. |
| `screenshots` | `[data-cs="website"] [data-part="screenshots"]` | Actual app captures with a sample library, linking to the full images. |
| `screenshot-main` | `[data-cs="website"] [data-part="screenshot-main"]` | The cropped desktop screenshot and its link to the full capture. |
| `screenshot-console` | `[data-cs="website"] [data-part="screenshot-console"]` | The cropped console screenshot and its link to the full capture. |
| `screenshot-label` | `[data-cs="website"] [data-part="screenshot-label"]` | A mode label below a screenshot. |
| `sources` | `[data-cs="website"] [data-part="sources"]` | Examples of supported library sources. |
| `video` | `[data-cs="website"] [data-part="video"]` | The introductory video and its direct YouTube link. |
| `video-player` | `[data-cs="website"] [data-part="video-player"]` | A responsive, lazy-loaded YouTube privacy-enhanced embed. |
| `features` | `[data-cs="website"] [data-part="features"]` | The library, household and emulation feature overview. |
| `section-heading` | `[data-cs="website"] [data-part="section-heading"]` | A section's introduction and heading. |
| `feature-grid` | `[data-cs="website"] [data-part="feature-grid"]` | The three feature descriptions. |
| `feature` | `[data-cs="website"] [data-part="feature"]` | One feature; data-feature is library, household or retro. |
| `feature-number` | `[data-cs="website"] [data-part="feature-number"]` | A decorative feature index. |
| `principles` | `[data-cs="website"] [data-part="principles"]` | Local ownership, offline use, privacy and customisation. |
| `principle-list` | `[data-cs="website"] [data-part="principle-list"]` | The three product principles. |
| `downloads` | `[data-cs="website"] [data-part="downloads"]` | Platform status and download links. |
| `download-grid` | `[data-cs="website"] [data-part="download-grid"]` | The three platform download panels. |
| `platform` | `[data-cs="website"] [data-part="platform"]` | A platform's status, formats and direct installer links. |
| `platform-title` | `[data-cs="website"] [data-part="platform-title"]` | Platform name, icon and optional status badge. |
| `badge` | `[data-cs="website"] [data-part="badge"]` | The Linux reference-platform label. |
| `release-note` | `[data-cs="website"] [data-part="release-note"]` | Download instructions and the release archive link. |
| `faq` | `[data-cs="website"] [data-part="faq"]` | Native details disclosures with common questions. |
| `closing` | `[data-cs="website"] [data-part="closing"]` | The final download invitation. |
| `footer` | `[data-cs="website"] [data-part="footer"]` | Project documentation, source and licence links. |

## `button`

A push button. Rendered by Button (apps/native/src/ui/button.rs); in the webview, Button.

Root: `[data-cs="button"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `icon` | `[data-cs="button"] [data-part="icon"]` | The leading icon. |

State on the root:

- `data-variant`: `primary`, `secondary`, `ghost`, `danger`
- `data-size`: `sm`, `md`

React Aria interaction: `data-hovered`, `data-pressed`, `data-focus-visible`, `data-disabled`, `data-focused`, `data-pending`

## `checkbox`

A checkbox with its label. Rendered by Checkbox (apps/native/src/ui/checkbox.rs); in the webview, Checkbox.

Root: `[data-cs="checkbox"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `box` | `[data-cs="checkbox"] [data-part="box"]` | The square. |
| `mark` | `[data-cs="checkbox"] [data-part="mark"]` | The tick or dash inside the box. |
| `label` | `[data-cs="checkbox"] [data-part="label"]` | The label. |

React Aria interaction: `data-hovered`, `data-pressed`, `data-focus-visible`, `data-disabled`, `data-selected`, `data-indeterminate`, `data-invalid`, `data-readonly`

## `segmented`

One choice from a few options shown side by side. Rendered by SegmentedControl (apps/native/src/ui/segmented.rs); in the webview, SegmentedControl.

Root: `[data-cs="segmented"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `segment` | `[data-cs="segmented"] [data-part="segment"]` | An option; carries `data-selected`, `data-hovered` and `data-focus-visible`. |
| `icon` | `[data-cs="segmented"] [data-part="icon"]` | A segment's icon. |

React Aria interaction: `data-disabled`, `data-orientation`

## `select`

A dropdown choice; its list opens in a `select-popover`. Rendered by Select (apps/native/src/ui/select.rs); in the webview, Select.

Root: `[data-cs="select"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `label` | `[data-cs="select"] [data-part="label"]` | The visible label, when given. |
| `trigger` | `[data-cs="select"] [data-part="trigger"]` | The button that opens the list; carries `data-hovered`, `data-pressed`, `data-focus-visible`. |
| `value` | `[data-cs="select"] [data-part="value"]` | The selected option's text. |
| `chevron` | `[data-cs="select"] [data-part="chevron"]` | The arrow on the trigger. |

React Aria interaction: `data-open`, `data-focused`, `data-focus-visible`, `data-disabled`, `data-invalid`

Contains: [`select-popover`](#select-popover)

## `select-popover`

The open list of a `select`, rendered in a portal. Rendered by Select (apps/native/src/ui/select.rs); in the webview, Select.

Root: `[data-cs="select-popover"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `option` | `[data-cs="select-popover"] [data-part="option"]` | An option; carries `data-focused` and `data-selected`. |
| `check` | `[data-cs="select-popover"] [data-part="check"]` | The tick on the selected option. |

React Aria interaction: `data-placement`, `data-entering`, `data-exiting`

## `menu`

A menu of actions, rendered in a portal: a button's dropdown, or a right click's menu at the pointer. Rendered by Menu, MenuButton (apps/native/src/ui/menu.rs); in the webview, MenuPopover, ContextMenu.

Root: `[data-cs="menu"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `list` | `[data-cs="menu"] [data-part="list"]` | The list of items. |
| `item` | `[data-cs="menu"] [data-part="item"]` | An action; carries `data-focused` and `data-disabled`, and `data-tone="danger"` when it removes something. |
| `icon` | `[data-cs="menu"] [data-part="icon"]` | An item's icon. |
| `label` | `[data-cs="menu"] [data-part="label"]` | An item's text. |
| `description` | `[data-cs="menu"] [data-part="description"]` | A second line under an item's text. |
| `section` | `[data-cs="menu"] [data-part="section"]` | A group of items. |
| `section-title` | `[data-cs="menu"] [data-part="section-title"]` | A group's heading. |
| `separator` | `[data-cs="menu"] [data-part="separator"]` | A line between groups. |

React Aria interaction: `data-placement`, `data-entering`, `data-exiting`

## `slider`

A value on a track. Rendered by Slider (apps/native/src/ui/slider.rs); in the webview, Slider.

Root: `[data-cs="slider"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `label` | `[data-cs="slider"] [data-part="label"]` | The visible label, when given. |
| `output` | `[data-cs="slider"] [data-part="output"]` | The value readout. |
| `track` | `[data-cs="slider"] [data-part="track"]` | The rail; carries `data-hovered`. |
| `fill` | `[data-cs="slider"] [data-part="fill"]` | The filled part of the rail. |
| `thumb` | `[data-cs="slider"] [data-part="thumb"]` | The handle; carries `data-dragging` and `data-focus-visible`. |

React Aria interaction: `data-disabled`, `data-orientation`

## `switch`

An on/off toggle. Rendered by Switch (apps/native/src/ui/switch.rs); in the webview, Switch.

Root: `[data-cs="switch"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `track` | `[data-cs="switch"] [data-part="track"]` | The pill. |
| `thumb` | `[data-cs="switch"] [data-part="thumb"]` | The knob. |
| `label` | `[data-cs="switch"] [data-part="label"]` | The label, when given. |

State on the root:

- `data-size`: `sm`, `md`

React Aria interaction: `data-hovered`, `data-pressed`, `data-focus-visible`, `data-disabled`, `data-selected`, `data-readonly`

## `text-field`

A single-line text input. Rendered by TextField, SearchField (apps/native/src/ui/text_field.rs); in the webview, TextField.

Root: `[data-cs="text-field"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `label` | `[data-cs="text-field"] [data-part="label"]` | The visible label, when given. |
| `control` | `[data-cs="text-field"] [data-part="control"]` | The box around the icon and input. |
| `icon` | `[data-cs="text-field"] [data-part="icon"]` | The leading icon. |
| `input` | `[data-cs="text-field"] [data-part="input"]` | The `<input>`; carries `data-focused` and `data-hovered`. |
| `action` | `[data-cs="text-field"] [data-part="action"]` | The control beside the input, e.g. a Browse… button. |
| `description` | `[data-cs="text-field"] [data-part="description"]` | The help text. |
| `error` | `[data-cs="text-field"] [data-part="error"]` | The validation message. |

Slots: `action` hold the caller's content; parts inside them belong to that content.

React Aria interaction: `data-disabled`, `data-invalid`, `data-readonly`, `data-required`

## `settings-section`

A titled group of settings. Rendered by SettingsSection (apps/native/src/ui/settings.rs); in the webview, SettingsSection.

Root: `[data-cs="settings-section"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `header` | `[data-cs="settings-section"] [data-part="header"]` | The heading row. |
| `icon` | `[data-cs="settings-section"] [data-part="icon"]` | The heading icon. |
| `title` | `[data-cs="settings-section"] [data-part="title"]` | The heading. |
| `description` | `[data-cs="settings-section"] [data-part="description"]` | The text under the heading. |
| `aside` | `[data-cs="settings-section"] [data-part="aside"]` | Badges or actions at the end of the heading row. |

Slots: `aside` hold the caller's content; parts inside them belong to that content.

Container: its content comes from the caller, which may add parts of its own.

Contains: [`setting-row`](#setting-row)

## `setting-row`

One setting: its label on the left, its controls on the right. Rendered by SettingRow (apps/native/src/ui/settings.rs); in the webview, SettingRow.

Root: `[data-cs="setting-row"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `icon` | `[data-cs="setting-row"] [data-part="icon"]` | The row icon. |
| `label` | `[data-cs="setting-row"] [data-part="label"]` | The setting's name. |
| `description` | `[data-cs="setting-row"] [data-part="description"]` | The text under the name. |
| `controls` | `[data-cs="setting-row"] [data-part="controls"]` | The controls. |

Slots: `controls` hold the caller's content; parts inside them belong to that content.

## `status-dot`

A small coloured dot and label, e.g. Detected / Not found. Rendered by StatusDot (apps/native/src/ui/settings.rs); in the webview, StatusDot.

Root: `[data-cs="status-dot"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `dot` | `[data-cs="status-dot"] [data-part="dot"]` | The dot. |
| `label` | `[data-cs="status-dot"] [data-part="label"]` | The text. |

State on the root:

- `data-tone`: `ok`, `off`, `warning`, `danger`

## `pill`

A bordered chip with an icon, e.g. Off by default. Rendered by Pill (apps/native/src/ui/settings.rs); in the webview, Pill.

Root: `[data-cs="pill"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `icon` | `[data-cs="pill"] [data-part="icon"]` | The icon. |
| `label` | `[data-cs="pill"] [data-part="label"]` | The text. |

State on the root:

- `data-tone`: `warning`, `danger`, `accent`

## `game-art`

A game's cover, or a blank placeholder when it has none. Rendered by GameArt (apps/native/src/ui/game_art.rs); in the webview, GameArt.

Root: `[data-cs="game-art"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `image` | `[data-cs="game-art"] [data-part="image"]` | The artwork image. |

State on the root:

- `data-source`: `image`, `placeholder`

## `status-badge`

A game's install status. Rendered by StatusBadge (apps/native/src/ui/status_badge.rs); in the webview, StatusBadge.

Root: `[data-cs="status-badge"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `icon` | `[data-cs="status-badge"] [data-part="icon"]` | The status icon. |
| `label` | `[data-cs="status-badge"] [data-part="label"]` | The status text. |

State on the root:

- `data-status`: `installed`, `not-installed`, `available`, `emulated`

## `game-card`

A game tile in a grid. Rendered by GameCard (apps/native/src/ui/game_card.rs); in the webview, GameCard; the library's LibraryCard and the Web tab's tiles (app).

Root: `[data-cs="game-card"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `art` | `[data-cs="game-card"] [data-part="art"]` | The cover (a `game-art`). |
| `badge` | `[data-cs="game-card"] [data-part="badge"]` | The status (a `status-badge`). |
| `title` | `[data-cs="game-card"] [data-part="title"]` | The game's title. |
| `subtitle` | `[data-cs="game-card"] [data-part="subtitle"]` | The genre; on LibraryCard, the platform unless it is a PC one. |
| `meta` | `[data-cs="game-card"] [data-part="meta"]` | The row of facts under the title. |
| `playtime` | `[data-cs="game-card"] [data-part="playtime"]` | Time played. |
| `size` | `[data-cs="game-card"] [data-part="size"]` | Size on disk. |
| `last-played` | `[data-cs="game-card"] [data-part="last-played"]` | When it was last played. |
| `stores` | `[data-cs="game-card"] [data-part="stores"]` | How many stores sell it. |
| `owned` | `[data-cs="game-card"] [data-part="owned"]` | Shown instead of stores when you own it. |
| `favorite` (app only) | `[data-cs="game-card"] [data-part="favorite"]` | The favourite heart. |
| `storage-availability` (app only) | `[data-cs="game-card"] [data-part="storage-availability"]` | Shown when the managed copy is unavailable. |
| `plugins` (app only) | `[data-cs="game-card"] [data-part="plugins"]` | Badges and overlays contributed by plugins. |
| `select` (app only) | `[data-cs="game-card"] [data-part="select"]` | In selection mode, the check that shows whether the game is chosen. |
| `playing-elsewhere` (app only) | `[data-cs="game-card"] [data-part="playing-elsewhere"]` | In console mode, the paired device the game is running on. |
| `played-on` (app only) | `[data-cs="game-card"] [data-part="played-on"]` | On Home's Recently Played, the other device the game was last played on, in place of the subtitle. |
| `starting` (app only) | `[data-cs="game-card"] [data-part="starting"]` | Starting…, over the art from a double click or Play until the game runs, the launch fails or a question stops it. |

Slots: `plugins` hold the caller's content; parts inside them belong to that content.

State on the root:

- `data-status`: `installed`, `not-installed`, `available`, `emulated`
- `data-variant`: `library`, `recent`, `store`, `web`
- `data-storage-status` (app only): `unavailable`
- `data-tile` (app only): `small`, `large`, `huge`
- `data-selected` (app only): `true`

Contains: [`game-art`](#game-art), [`status-badge`](#status-badge)

## `hero`

The large featured-game banner. Rendered by Hero (apps/native/src/ui/hero.rs); the home and game pages' hero sections (app); in the webview, Hero; the home and game pages' hero sections (app).

Root: `[data-cs="hero"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `art` (app only) | `[data-cs="hero"] [data-part="art"]` | The game's artwork (a `game-art`). |
| `glow` | `[data-cs="hero"] [data-part="glow"]` | The coloured glow behind the content. |
| `shade` | `[data-cs="hero"] [data-part="shade"]` | The gradient that keeps text readable. |
| `fx` | `[data-cs="hero"] [data-part="fx"]` | The scanline and beam overlay. |
| `content` | `[data-cs="hero"] [data-part="content"]` | The text and actions. |
| `eyebrow` | `[data-cs="hero"] [data-part="eyebrow"]` | The small label above the title. |
| `live-dot` | `[data-cs="hero"] [data-part="live-dot"]` | The pulsing dot in the eyebrow. |
| `title` | `[data-cs="hero"] [data-part="title"]` | The game's title. |
| `description` | `[data-cs="hero"] [data-part="description"]` | The game's description. |
| `meta` | `[data-cs="hero"] [data-part="meta"]` | Genre, size and playtime. |
| `actions` | `[data-cs="hero"] [data-part="actions"]` | The buttons. |
| `primary-action` | `[data-cs="hero"] [data-part="primary-action"]` | The launch button group. |
| `badge` | `[data-cs="hero"] [data-part="badge"]` | The status (a `status-badge`). |
| `tags` (app only) | `[data-cs="hero"] [data-part="tags"]` | The game's tags. |
| `website` (app only) | `[data-cs="hero"] [data-part="website"]` | A link to the game's own page on the web (a web game's). |
| `played-on` (app only) | `[data-cs="hero"] [data-part="played-on"]` | On Home, the other device the game was last played on (in the meta). |

Slots: `actions` hold the caller's content; parts inside them belong to that content.

Contains: [`game-art`](#game-art), [`status-badge`](#status-badge)

## `shelf`

A titled row of cards. Rendered by Shelf (apps/native/src/ui/shelf.rs); in the webview, Shelf.

Root: `[data-cs="shelf"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `header` | `[data-cs="shelf"] [data-part="header"]` | The heading row. |
| `icon` | `[data-cs="shelf"] [data-part="icon"]` | The heading icon. |
| `title` | `[data-cs="shelf"] [data-part="title"]` | The heading. |
| `subtitle` | `[data-cs="shelf"] [data-part="subtitle"]` | The text beside the heading. |
| `view-all` | `[data-cs="shelf"] [data-part="view-all"]` | The View All button. |
| `items` | `[data-cs="shelf"] [data-part="items"]` | The grid of cards. |

Slots: `items` hold the caller's content; parts inside them belong to that content.

Contains: [`game-card`](#game-card)

## `avatar`

An account's picture: one of the shipped avatars, or the name's initials when its id is unknown or the image can't load. Rendered by Avatar (apps/native/src/ui/avatar.rs); in the webview, Avatar.

Root: `[data-cs="avatar"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `image` | `[data-cs="avatar"] [data-part="image"]` | The avatar image. |
| `initials` | `[data-cs="avatar"] [data-part="initials"]` | The initials shown instead. |

State on the root:

- `data-size`: `sm`, `md`, `lg`, `xl`
- `data-source`: `image`, `initials`

## `error-notice`

A failure, shown the same way everywhere: what failed and why, what to do, and the technical details behind a toggle. Rendered by ErrorNotice (apps/native/src/ui/error_notice.rs); in the webview, ErrorNotice.

Root: `[data-cs="error-notice"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `icon` | `[data-cs="error-notice"] [data-part="icon"]` | The alert icon. |
| `title` | `[data-cs="error-notice"] [data-part="title"]` | What couldn't be done, when given. |
| `summary` | `[data-cs="error-notice"] [data-part="summary"]` | What failed and, when known, why. |
| `hints` | `[data-cs="error-notice"] [data-part="hints"]` | What the user can do, one item each. |
| `actions` | `[data-cs="error-notice"] [data-part="actions"]` | Recovery controls (e.g. Try again) and the details toggle. |
| `details-toggle` | `[data-cs="error-notice"] [data-part="details-toggle"]` | Show details / Hide details; carries `aria-expanded`. |
| `details` | `[data-cs="error-notice"] [data-part="details"]` | The redacted technical details and the log reference. |
| `reference` | `[data-cs="error-notice"] [data-part="reference"]` | The reference to find the failure in the log. |

Slots: `actions` hold the caller's content; parts inside them belong to that content.

State on the root:

- `data-variant`: `block`, `inline`
- `data-tone`: `danger`, `warning`
- `data-expanded`: `true`, `false`

Contains: [`button`](#button)

## `avatar-picker`

A grid of the shipped avatars to choose one from (a radio group). Rendered by AvatarPicker (apps/native/src/ui/avatar.rs); in the webview, AvatarPicker.

Root: `[data-cs="avatar-picker"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `option` | `[data-cs="avatar-picker"] [data-part="option"]` | An avatar to choose (an `avatar` inside); carries `data-selected`, `data-hovered` and `data-focus-visible`. |
| `check` | `[data-cs="avatar-picker"] [data-part="check"]` | The tick on the chosen avatar. |

State on the root:

- `data-size`: `md`, `lg`, `xl`

React Aria interaction: `data-disabled`, `data-orientation`

Contains: [`avatar`](#avatar)

## `app-shell`

The window: sidebar, topbar, current view and persistent status bar. Rendered by Frame (apps/native/src/frame/mod.rs); in the webview, AppShell (app).

Root: `[data-cs="app-shell"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `main` | `[data-cs="app-shell"] [data-part="main"]` | The column right of the sidebar. |

Container: its content comes from the caller, which may add parts of its own.

Contains: [`backdrop`](#backdrop), [`sidebar`](#sidebar), [`topbar`](#topbar), [`view`](#view), [`status-bar`](#status-bar)

## `status-bar`

Persistent background activity, application version and an available Caskade update. IGDB and Hasheous are credited in Settings → About (`about`). Rendered by StatusBar (apps/native/src/frame/status_bar.rs); in the webview, StatusBar (app).

Root: `[data-cs="status-bar"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `spinner` | `[data-cs="status-bar"] [data-part="spinner"]` | The activity spinner; hidden while idle. |
| `activity` | `[data-cs="status-bar"] [data-part="activity"]` | A concise description of current work or queue status. |
| `version` | `[data-cs="status-bar"] [data-part="version"]` | The application version. |
| `update` | `[data-cs="status-bar"] [data-part="update"]` | A link to Settings → General once a check finds a new Caskade version; `data-status` is `available`, or `ready` once it is installed and waits for a restart. |
| `sync` | `[data-cs="status-bar"] [data-part="sync"]` | With paired devices (#231): how many are in reach and the best way one is reached (a `peer-link`), linked to Settings → Devices; `data-status` is `lan`, `direct`, `relay`, `offline` or `syncing`. |

## `backdrop`

The decorative layer behind everything. Rendered by Frame (apps/native/src/frame/mod.rs); in the webview, AppShell (app).

Root: `[data-cs="backdrop"]`

## `onboarding`

First-run setup, shown in place of the app shell: a welcome, the questions (`language`, `folder`, `sources`, `mode`, and `account`, the admin's name, avatar and PIN), then the work they start (`setup`). Things arriving use `animate-rise`; the running task's sweep pauses with `--cs-motion-ambient`. Rendered by Onboarding (apps/native/src/onboarding/mod.rs); in the webview, Onboarding (app).

Root: `[data-cs="onboarding"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `titlebar` | `[data-cs="onboarding"] [data-part="titlebar"]` | The row at the top that moves the window. |
| `sound` | `[data-cs="onboarding"] [data-part="sound"]` | Turns setup's sounds on and off. |
| `skip` | `[data-cs="onboarding"] [data-part="skip"]` | Skips setup, keeping the defaults. |
| `main` | `[data-cs="onboarding"] [data-part="main"]` | The current step. |
| `welcome` | `[data-cs="onboarding"] [data-part="welcome"]` | The first screen. |
| `logo` | `[data-cs="onboarding"] [data-part="logo"]` | The logo on the welcome screen, with its glow. |
| `start` | `[data-cs="onboarding"] [data-part="start"]` | Get started. |
| `question` | `[data-cs="onboarding"] [data-part="question"]` | A question; `data-question` names it. |
| `card` | `[data-cs="onboarding"] [data-part="card"]` | The games folder, its free space and its Change button. |
| `choose` | `[data-cs="onboarding"] [data-part="choose"]` | Chooses another games folder. |
| `source` | `[data-cs="onboarding"] [data-part="source"]` | Something to look for; `data-status` is `on` or `off`. |
| `mode` | `[data-cs="onboarding"] [data-part="mode"]` | Desktop or console mode; React Aria's `data-selected` marks the choice. |
| `language` | `[data-cs="onboarding"] [data-part="language"]` | A language, or System; React Aria's `data-selected` marks the choice. |
| `next` | `[data-cs="onboarding"] [data-part="next"]` | Continue, Set up, or Open my library. |
| `progress` | `[data-cs="onboarding"] [data-part="progress"]` | The step dots; each has `data-status` `active`, `done` or `next`. |
| `setup` | `[data-cs="onboarding"] [data-part="setup"]` | The work being done; `data-status` is `running` or `done`. |
| `covers` | `[data-cs="onboarding"] [data-part="covers"]` | Covers of the games found so far, behind the list. |
| `count` | `[data-cs="onboarding"] [data-part="count"]` | How many games the library has. |
| `tasks` | `[data-cs="onboarding"] [data-part="tasks"]` | The list of tasks. |
| `task` | `[data-cs="onboarding"] [data-part="task"]` | One task; `data-task` names it, `data-status` is `waiting`, `running`, `done`, `warning` or `failed`. |

State on the root:

- `data-step`: `welcome`, `language`, `folder`, `sources`, `mode`, `account`, `setup`

Contains: [`backdrop`](#backdrop), [`window-controls`](#window-controls), [`button`](#button), [`switch`](#switch), [`checkbox`](#checkbox), [`text-field`](#text-field), [`game-art`](#game-art), [`avatar-picker`](#avatar-picker)

## `sidebar`

The navigation column. Rendered by Sidebar (apps/native/src/frame/sidebar.rs); in the webview, Sidebar (app).

Root: `[data-cs="sidebar"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `brand` | `[data-cs="sidebar"] [data-part="brand"]` | The logo and name. |
| `nav` | `[data-cs="sidebar"] [data-part="nav"]` | A group of links. |
| `nav-item` | `[data-cs="sidebar"] [data-part="nav-item"]` | A link; the current one has `data-status="active"`. |
| `icon` | `[data-cs="sidebar"] [data-part="icon"]` | A link's icon. |
| `label` | `[data-cs="sidebar"] [data-part="label"]` | A link's text. |
| `divider` | `[data-cs="sidebar"] [data-part="divider"]` | The rule between the groups. |
| `sub-nav` | `[data-cs="sidebar"] [data-part="sub-nav"]` | The links under a link: the Library's filters, open (`data-open`) while the Library is. |
| `sub-item` | `[data-cs="sidebar"] [data-part="sub-item"]` | A filter; the current one has `data-status="active"`. |
| `count` | `[data-cs="sidebar"] [data-part="count"]` | How many games a filter has. |
| `plugin-nav` | `[data-cs="sidebar"] [data-part="plugin-nav"]` | The links to plugin views. |

Slots: `plugin-nav` hold the caller's content; parts inside them belong to that content.

## `topbar`

The bar above the view: search, quick actions and the window controls. It is the window's title bar: dragging it moves the window. Rendered by Topbar (apps/native/src/frame/topbar.rs); in the webview, Topbar (app).

Root: `[data-cs="topbar"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `actions` | `[data-cs="topbar"] [data-part="actions"]` | The items at the end of the bar. |
| `activity` | `[data-cs="topbar"] [data-part="activity"]` | The link to Activity. |
| `job-count` | `[data-cs="topbar"] [data-part="job-count"]` | The number of running jobs; `data-status` is `active` or `needs-user`. |
| `notification-count` | `[data-cs="topbar"] [data-part="notification-count"]` | The number of unread notifications, apart from the jobs. |
| `avatar` | `[data-cs="topbar"] [data-part="avatar"]` | The signed-in account's button, which opens the account menu (an `avatar` inside). |
| `console` | `[data-cs="topbar"] [data-part="console"]` | Enters console mode, where Caskade draws no window controls (macOS, the browser preview). |

Slots: `actions` hold the caller's content; parts inside them belong to that content.

Contains: [`search`](#search), [`window-controls`](#window-controls), [`avatar`](#avatar)

## `window-controls`

Minimize, maximize and close at the end of the topbar (Windows and Linux; macOS keeps its traffic lights). Each button carries `data-hovered` and `data-pressed`. Rendered by WindowControls (apps/native/src/frame/window.rs); in the webview, WindowControls (app).

Root: `[data-cs="window-controls"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `minimize` | `[data-cs="window-controls"] [data-part="minimize"]` | Minimizes the window. |
| `console` | `[data-cs="window-controls"] [data-part="console"]` | Enters console mode (fullscreen, for a controller). |
| `maximize` | `[data-cs="window-controls"] [data-part="maximize"]` | Maximizes the window, or restores it when maximized. |
| `close` | `[data-cs="window-controls"] [data-part="close"]` | Closes the window. |

State on the root:

- `data-status`: `restored`, `maximized`

## `view-error`

Shown in place of a view that failed to render, with the shell around it; or in place of the whole window when the shell itself failed, with the window controls. Rendered by ViewError, AppCrash (apps/native/src/frame/error.rs); in the webview, ViewError, AppCrash (app).

Root: `[data-cs="view-error"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `title` | `[data-cs="view-error"] [data-part="title"]` | The heading. |
| `message` | `[data-cs="view-error"] [data-part="message"]` | The error's text. |
| `actions` | `[data-cs="view-error"] [data-part="actions"]` | Try again and Open the library, or Reload. |

## `console-shell`

Console mode's window (`[data-mode="console"]` on the root): the console sidebar, top bar and view, and the button-hint bar. `data-cursor` is `hidden` while the pointer rests. Rendered by ConsoleShell (app).

Root: `[data-cs="console-shell"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `main` | `[data-cs="console-shell"] [data-part="main"]` | The column right of the sidebar. |

Container: its content comes from the caller, which may add parts of its own.

Contains: [`backdrop`](#backdrop), [`console-sidebar`](#console-sidebar), [`console-topbar`](#console-topbar), [`view`](#view), [`hint-bar`](#hint-bar)

## `console-sidebar`

Console mode's navigation: big targets. Rendered by ConsoleSidebar (app).

Root: `[data-cs="console-sidebar"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `brand` | `[data-cs="console-sidebar"] [data-part="brand"]` | The logo and name. |
| `nav` | `[data-cs="console-sidebar"] [data-part="nav"]` | The links. |
| `nav-item` | `[data-cs="console-sidebar"] [data-part="nav-item"]` | A link; the current one has `data-status="active"`. |
| `icon` | `[data-cs="console-sidebar"] [data-part="icon"]` | A link's icon. |
| `label` | `[data-cs="console-sidebar"] [data-part="label"]` | A link's text. |
| `sub-nav` | `[data-cs="console-sidebar"] [data-part="sub-nav"]` | The links under a link: the Library's filters, open (`data-open`) while the Library is. |
| `sub-item` | `[data-cs="console-sidebar"] [data-part="sub-item"]` | A filter; the current one has `data-status="active"`. |
| `count` | `[data-cs="console-sidebar"] [data-part="count"]` | How many games a filter has. |
| `plugin-nav` | `[data-cs="console-sidebar"] [data-part="plugin-nav"]` | Plugin views that show in console mode. |
| `leave` | `[data-cs="console-sidebar"] [data-part="leave"]` | Goes back to desktop mode. |

Slots: `plugin-nav` hold the caller's content; parts inside them belong to that content.

## `console-topbar`

Console mode's top bar: search and activity. Rendered by ConsoleTopbar (app).

Root: `[data-cs="console-topbar"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `actions` | `[data-cs="console-topbar"] [data-part="actions"]` | The items at the end of the bar. |
| `activity` | `[data-cs="console-topbar"] [data-part="activity"]` | The link to Activity. |
| `job-count` | `[data-cs="console-topbar"] [data-part="job-count"]` | The number of running jobs; `data-status` is `active` or `needs-user`. |
| `notification-count` | `[data-cs="console-topbar"] [data-part="notification-count"]` | The number of unread notifications, apart from the jobs. |
| `avatar` | `[data-cs="console-topbar"] [data-part="avatar"]` | The signed-in account's button, which opens the account menu (an `avatar` inside). |

Contains: [`search`](#search), [`select`](#select), [`avatar`](#avatar)

## `notifications`

Activity's notification feed (#198): game additions in hourly batches, failed jobs, updates on offer and plugins turned off, newest change first. Rendered by Feed (apps/native/src/frame/notifications.rs); in the webview, NotificationFeed (app).

Root: `[data-cs="notifications"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `toolbar` | `[data-cs="notifications"] [data-part="toolbar"]` | The filters and Mark all read. |
| `filter` | `[data-cs="notifications"] [data-part="filter"]` | All or Unread; the current one has `data-selected`. |
| `mark-all` | `[data-cs="notifications"] [data-part="mark-all"]` | Marks read every item shown so far. |
| `empty` | `[data-cs="notifications"] [data-part="empty"]` | Shown when the feed (or its unread filter) is empty. |
| `pages` | `[data-cs="notifications"] [data-part="pages"]` | Newer and Older, when the feed has more than a page. |

Contains: [`notification`](#notification), [`button`](#button)

## `notification`

One item of the notification feed. `data-category` is what it is about; `data-resolved` is set once its problem has cleared. Rendered by Item (apps/native/src/frame/notifications.rs); in the webview, NotificationFeed (app).

Root: `[data-cs="notification"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `indicator` | `[data-cs="notification"] [data-part="indicator"]` | The unread dot. |
| `meta` | `[data-cs="notification"] [data-part="meta"]` | Its category and time. |
| `time` | `[data-cs="notification"] [data-part="time"]` | When: a batch's first to last addition, else its last change. |
| `title` | `[data-cs="notification"] [data-part="title"]` | The heading. |
| `body` | `[data-cs="notification"] [data-part="body"]` | Details: a batch's sources, a failure's error. |
| `actions` | `[data-cs="notification"] [data-part="actions"]` | Open (the list, the game or the settings it is about) and Mark read or unread. |
| `members` | `[data-cs="notification"] [data-part="members"]` | What it is about, once opened: a batch's games, the updates on offer. |
| `member` | `[data-cs="notification"] [data-part="member"]` | One of them; `data-status` is `present`, `merged` (it links to the game it joined), `removed` or `other`. |

State on the root:

- `data-status`: `unread`, `read`
- `data-severity`: `info`, `warning`, `error`
- `data-category`: `gamesAdded`, `jobFailed`, `updatesAvailable`, `pluginDisabled`, `achievementsUnlocked`
- `data-resolved`: `true`

Contains: [`button`](#button)

## `hint-bar`

What each controller button does where focus is. `data-glyphs` is the controller family in hand: `xbox`, `playstation`, `nintendo`, `generic`, or `keyboard`. Rendered by HintBar (app).

Root: `[data-cs="hint-bar"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `hint` | `[data-cs="hint-bar"] [data-part="hint"]` | One button and what it does; `data-action` is the action (`confirm`, `back`, `tab.next`, …). |
| `glyph` | `[data-cs="hint-bar"] [data-part="glyph"]` | The button's label; face buttons carry `data-tone` (`installed`, `danger`, `accent`, `warning`). |
| `label` | `[data-cs="hint-bar"] [data-part="label"]` | What the button does. |
| `clock` | `[data-cs="hint-bar"] [data-part="clock"]` | The time. |

## `game-curtain`

Console mode's black screen behind a running game: it fades in when a game starts, so the game's window takes over without Caskade minimizing, and fades out when the last game exits. Always present in console mode; `data-state` is `covered` while it is down. Rendered by GameCurtain (app).

Root: `[data-cs="game-curtain"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `title` | `[data-cs="game-curtain"] [data-part="title"]` | The game's title. |
| `hint` | `[data-cs="game-curtain"] [data-part="hint"]` | The guide button, which opens the quick menu over it. |

State on the root:

- `data-state`: `covered`, `open`

## `shelf-row`

A row of game cards that pages with LB/RB (Home's shelves). Rendered by ShelfRow (apps/native/src/home/shelf_row.rs); in the webview, ShelfRow (app).

Root: `[data-cs="shelf-row"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `header` | `[data-cs="shelf-row"] [data-part="header"]` | The title row. |
| `icon` | `[data-cs="shelf-row"] [data-part="icon"]` | The shelf's icon. |
| `title` | `[data-cs="shelf-row"] [data-part="title"]` | The shelf's name. |
| `pager` | `[data-cs="shelf-row"] [data-part="pager"]` | The LB/RB buttons, in console mode. |
| `view-all` | `[data-cs="shelf-row"] [data-part="view-all"]` | The link to every game in the shelf. |
| `items` | `[data-cs="shelf-row"] [data-part="items"]` | The row of cards. |

Slots: `items` hold the caller's content; parts inside them belong to that content.

## `search`

The game search; suggestions open in a `search-popover`. Rendered by SearchBox (apps/native/src/frame/search.rs); in the webview, SearchBox (app).

Root: `[data-cs="search"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `control` | `[data-cs="search"] [data-part="control"]` | The box around the input; carries `data-focus-within`. |
| `icon` | `[data-cs="search"] [data-part="icon"]` | The magnifier. |
| `input` | `[data-cs="search"] [data-part="input"]` | The `<input>`. |
| `shortcut` | `[data-cs="search"] [data-part="shortcut"]` | The shortcut that focuses it: Ctrl K, or in console mode the controller's search button. |

React Aria interaction: `data-open`, `data-disabled`

Contains: [`search-popover`](#search-popover)

## `search-popover`

The search suggestions, rendered in a portal. Rendered by SearchBox (apps/native/src/frame/search.rs); in the webview, SearchBox (app).

Root: `[data-cs="search-popover"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `suggestion` | `[data-cs="search-popover"] [data-part="suggestion"]` | A suggested game; carries `data-focused`. |
| `see-all` | `[data-cs="search-popover"] [data-part="see-all"]` | The item that opens every result in the library. |

React Aria interaction: `data-placement`, `data-entering`, `data-exiting`

Contains: [`game-art`](#game-art), [`status-badge`](#status-badge)

## `view`

The scrolling area that shows the current page. Rendered by Frame (apps/native/src/frame/mod.rs); in the webview, AppShell (app).

Root: `[data-cs="view"]`

Container: its content comes from the caller, which may add parts of its own.

## `settings-nav`

Settings' sections: a vertical tab list beside the open section's panel, in desktop and console modes, grouped under headings, with the settings search above it. It stays in view while the panel scrolls, and scrolls on its own when the sections don't fit. Rendered by SettingsView (apps/native/src/settings/mod.rs); in the webview, SettingsView (app).

Root: `[data-cs="settings-nav"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `group` | `[data-cs="settings-nav"] [data-part="group"]` | A group's heading, before its sections (App, Library, …); not a stop for focus. |
| `tab` | `[data-cs="settings-nav"] [data-part="tab"]` | A section; the open one carries `data-selected`, and each `data-hovered`, `data-pressed` and `data-focus-visible`. |
| `icon` | `[data-cs="settings-nav"] [data-part="icon"]` | A section's icon. |
| `label` | `[data-cs="settings-nav"] [data-part="label"]` | A section's name; long names wrap. |

React Aria interaction: `data-orientation`

## `about`

Settings → About: the IGDB and Hasheous credits first, side by side, then the app, its version and build, project links, and the links into `legal`. Rendered by AboutPage (apps/native/src/settings/about.rs); in the webview, AboutPage (app).

Root: `[data-cs="about"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `title` | `[data-cs="about"] [data-part="title"]` | The page heading. |
| `credits` | `[data-cs="about"] [data-part="credits"]` | The list of data credits, directly under the heading. |
| `credit` | `[data-cs="about"] [data-part="credit"]` | One credit (IGDB, Hasheous): a link to the service, each with the same weight. |
| `details` | `[data-cs="about"] [data-part="details"]` | The app's name, logo and short description. |
| `version` | `[data-cs="about"] [data-part="version"]` | The version and build, from the app's build metadata. |
| `links` | `[data-cs="about"] [data-part="links"]` | Links to the project page, issue tracker and releases. |
| `legal-links` | `[data-cs="about"] [data-part="legal-links"]` | Links to each section of the Legal view. |

## `legal`

Settings → About → Legal: licences, open-source packages, terms and privacy policy, bundled for offline reading. One section shows at a time, at `/settings?tab=about&section=<id>`. Rendered by LegalView (apps/native/src/settings/about.rs); in the webview, LegalView (app).

Root: `[data-cs="legal"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `back` | `[data-cs="legal"] [data-part="back"]` | The link back to About. |
| `title` | `[data-cs="legal"] [data-part="title"]` | The view's heading. |
| `nav` | `[data-cs="legal"] [data-part="nav"]` | The section links. |
| `section-link` | `[data-cs="legal"] [data-part="section-link"]` | A section; the open one carries `aria-current`. |
| `document` | `[data-cs="legal"] [data-part="document"]` | A long text (a policy, a licence group's packages and texts): focusable, so keys and a pad scroll it. |

## `system-stats`

Settings → Statistics: this machine's CPU, GPUs, memory, network and storage capacity, live, as the same cards in desktop and console modes. `data-status` says whether readings are coming in. Rendered by StatisticsDashboard (apps/native/src/settings/statistics.rs); in the webview, StatisticsDashboard (app).

Root: `[data-cs="system-stats"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `title` | `[data-cs="system-stats"] [data-part="title"]` | The section heading. |
| `status` | `[data-cs="system-stats"] [data-part="status"]` | The line under it: live, waiting, not updating or unavailable, in words. |
| `card` | `[data-cs="system-stats"] [data-part="card"]` | One metric (`data-metric`: `cpu`, `memory`, `gpu`, `network`, `storage`); focusable, so keys and a pad reach every card. |
| `gauge` | `[data-cs="system-stats"] [data-part="gauge"]` | A utilization gauge (`role=meter`) with its percentage written inside. |
| `graph` | `[data-cs="system-stats"] [data-part="graph"]` | A recent-history graph; a missed sample is a gap in its line. |
| `unavailable` | `[data-cs="system-stats"] [data-part="unavailable"]` | Why a metric has no value: unsupported, no device or a failed sample. |
| `interface` | `[data-cs="system-stats"] [data-part="interface"]` | A network interface and its rates; `data-kind` is `physical`, `virtual` or `loopback`. |
| `volume` | `[data-cs="system-stats"] [data-part="volume"]` | A storage volume: its name, mount points and capacity. |
| `capacity` | `[data-cs="system-stats"] [data-part="capacity"]` | A volume's used-space bar (`role=meter`). |

State on the root:

- `data-status`: `loading`, `live`, `stale`, `error`

## `panel`

A framed panel inside a view. Rendered by CollectionsView (apps/native/src/collections/list.rs), CollectionView (apps/native/src/collections/page.rs); in the webview, Collection, Collections and Emulation views (app).

Root: `[data-cs="panel"]`

Container: its content comes from the caller, which may add parts of its own.

## `collection-card`

A collection tile. Rendered by CollectionCard (apps/native/src/collections/card.rs); in the webview, CollectionCard (app).

Root: `[data-cs="collection-card"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `art` | `[data-cs="collection-card"] [data-part="art"]` | The mosaic of its games. |
| `badge` | `[data-cs="collection-card"] [data-part="badge"]` | Smart or manual. |
| `pin` | `[data-cs="collection-card"] [data-part="pin"]` | Shown on pinned collections. |
| `count` | `[data-cs="collection-card"] [data-part="count"]` | How many games it holds. |
| `title` | `[data-cs="collection-card"] [data-part="title"]` | The collection's name. |
| `subtitle` | `[data-cs="collection-card"] [data-part="subtitle"]` | Its description. |
| `meta` | `[data-cs="collection-card"] [data-part="meta"]` | Playtime and when it changed. |

State on the root:

- `data-kind`: `smart`, `manual`

## `platform-card`

A platform in the library's breakdown of emulated games; opens its games. Rendered by PlatformGrid (apps/native/src/library/platform_grid.rs); in the webview, PlatformGrid (app).

Root: `[data-cs="platform-card"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `art` | `[data-cs="platform-card"] [data-part="art"]` | The hardware render on its glow. |
| `title` | `[data-cs="platform-card"] [data-part="title"]` | The platform's short name. |
| `subtitle` | `[data-cs="platform-card"] [data-part="subtitle"]` | Its maker and kind of machine. |
| `count` | `[data-cs="platform-card"] [data-part="count"]` | How many games it has. |
| `meta` | `[data-cs="platform-card"] [data-part="meta"]` | Playtime and last play. |
| `playtime` | `[data-cs="platform-card"] [data-part="playtime"]` | Time played on it. |
| `last-played` | `[data-cs="platform-card"] [data-part="last-played"]` | When one of its games was last played. |

State on the root:

- `data-art`: `platform`, `generic`

## `game-row`

A game in the library's compact table (the list layout). Rendered by GameRow (apps/native/src/ui/game_row.rs); in the webview, LibraryRow (app).

Root: `[data-cs="game-row"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `icon` | `[data-cs="game-row"] [data-part="icon"]` | The game's icon, or its initial on generated art. |
| `title` | `[data-cs="game-row"] [data-part="title"]` | The game's title; links to the game and covers the row. |
| `favorite` | `[data-cs="game-row"] [data-part="favorite"]` | The favourite heart. |
| `storage-availability` | `[data-cs="game-row"] [data-part="storage-availability"]` | Shown when the managed copy is unavailable. |
| `plugins` | `[data-cs="game-row"] [data-part="plugins"]` | Badges contributed by plugins. |
| `badge` | `[data-cs="game-row"] [data-part="badge"]` | The status (a `status-badge`). |
| `last-played` | `[data-cs="game-row"] [data-part="last-played"]` | When it was last played. |
| `playtime` | `[data-cs="game-row"] [data-part="playtime"]` | Time played. |
| `size` | `[data-cs="game-row"] [data-part="size"]` | Size on disk. |
| `actions` | `[data-cs="game-row"] [data-part="actions"]` | The actions menu button. |
| `select` | `[data-cs="game-row"] [data-part="select"]` | In selection mode, the check that shows whether the game is chosen. |
| `starting` | `[data-cs="game-row"] [data-part="starting"]` | Starting…, after the title from a double click or Play until the game runs, the launch fails or a question stops it. |

Slots: `plugins` hold the caller's content; parts inside them belong to that content.

State on the root:

- `data-status`: `installed`, `not-installed`, `emulated`
- `data-storage-status`: `unavailable`
- `data-selected`: `true`

Contains: [`status-badge`](#status-badge)

## `alphabet-bar`

The letters beside a long library sorted by title; each jumps to its games, and a drag jumps letter by letter. Rendered by AlphabetBar (app).

Root: `[data-cs="alphabet-bar"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `letter` | `[data-cs="alphabet-bar"] [data-part="letter"]` | One initial (`#` for numbers and symbols); `data-selected` on the letter on screen, `data-disabled` when no game starts with it. |

## `quick-look`

A game at a glance over the current view: in desktop mode a column on the right that a click on a game opens, in console mode a modal from a pad's X on a card; X again plays it. Rendered by QuickLook (apps/native/src/game_menu/quick_look.rs); in the webview, QuickLook (app).

Root: `[data-cs="quick-look"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `art` | `[data-cs="quick-look"] [data-part="art"]` | The hero or cover (a `game-art`). |
| `close` | `[data-cs="quick-look"] [data-part="close"]` | Closes it (a `button`). |
| `platform` | `[data-cs="quick-look"] [data-part="platform"]` | Its platforms. |
| `title` | `[data-cs="quick-look"] [data-part="title"]` | The game's title. |
| `actions` | `[data-cs="quick-look"] [data-part="actions"]` | Play, and View more beneath it. |
| `play` | `[data-cs="quick-look"] [data-part="play"]` | Play, or Stop while it runs (a `button`). |
| `open` | `[data-cs="quick-look"] [data-part="open"]` | View more: opens the game's page (a `button`). |
| `notice` | `[data-cs="quick-look"] [data-part="notice"]` | Why it can't be played. |
| `facts` | `[data-cs="quick-look"] [data-part="facts"]` | Playtime, last play, size and release. |
| `playtime` | `[data-cs="quick-look"] [data-part="playtime"]` | Time played. |
| `last-played` | `[data-cs="quick-look"] [data-part="last-played"]` | When it was last played. |
| `size` | `[data-cs="quick-look"] [data-part="size"]` | Its size on disk. |
| `released` | `[data-cs="quick-look"] [data-part="released"]` | Its release date. |
| `genres` | `[data-cs="quick-look"] [data-part="genres"]` | Its genres and tags. |
| `description` | `[data-cs="quick-look"] [data-part="description"]` | The game's description. |

Contains: [`game-art`](#game-art), [`status-badge`](#status-badge), [`button`](#button)

## `game-editor`

A game's editor, from its menu's Edit… or its page's Edit details: its details as one draft saved together, its artwork, and tabs for its launch actions and scripts. A dialog in desktop mode, the whole window in console mode. Rendered by GameEditor (apps/native/src/game/editor/mod.rs); in the webview, GameEditor (app).

Root: `[data-cs="game-editor"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `title` | `[data-cs="game-editor"] [data-part="title"]` | The editor's heading. |
| `tabs` | `[data-cs="game-editor"] [data-part="tabs"]` | The editor's sections: Details, Actions, Scripts. |
| `tab` | `[data-cs="game-editor"] [data-part="tab"]` | One section; `data-selected` on the open one, `data-dirty` while it holds unsaved changes. |
| `details` | `[data-cs="game-editor"] [data-part="details"]` | The Details form. |
| `field` | `[data-cs="game-editor"] [data-part="field"]` | One field, with `data-field` naming it (`title`, `description`, `tags`, …), `data-manual` when its value is the user's and refreshes keep it, and `data-invalid` when the last save was refused for it. |
| `label` | `[data-cs="game-editor"] [data-part="label"]` | A field's name. |
| `manual` | `[data-cs="game-editor"] [data-part="manual"]` | Marks a value the user set. |
| `reset` | `[data-cs="game-editor"] [data-part="reset"]` | Puts a field or an artwork kind back to its automatic value (a `button`). |
| `error` | `[data-cs="game-editor"] [data-part="error"]` | Why a field wasn't saved, with the conflict choices. |
| `use-theirs` | `[data-cs="game-editor"] [data-part="use-theirs"]` | Takes the value saved meanwhile (a `button`). |
| `keep-mine` | `[data-cs="game-editor"] [data-part="keep-mine"]` | Keeps the user's value over the one saved meanwhile (a `button`). |
| `list` | `[data-cs="game-editor"] [data-part="list"]` | A list field's values and its add field. |
| `chip` | `[data-cs="game-editor"] [data-part="chip"]` | One value of a list. |
| `remove` | `[data-cs="game-editor"] [data-part="remove"]` | Removes a value from its list (a `button`). |
| `footer` | `[data-cs="game-editor"] [data-part="footer"]` | Status, Cancel and Save. |
| `status` | `[data-cs="game-editor"] [data-part="status"]` | Saving, or what went wrong. |
| `choice` | `[data-cs="game-editor"] [data-part="choice"]` | Who decides an artwork kind's image: automatic, chosen by the user, or none. |
| `artwork` | `[data-cs="game-editor"] [data-part="artwork"]` | The Artwork tab. |
| `slot` | `[data-cs="game-editor"] [data-part="slot"]` | One kind of artwork (`data-kind`: `cover`, `hero`, `icon`; `data-choice`: `automatic`, `chosen`, `removed`; `data-drop-active` while an image is dragged over it). |
| `preview` | `[data-cs="game-editor"] [data-part="preview"]` | The image that kind shows now. |
| `candidate` | `[data-cs="game-editor"] [data-part="candidate"]` | An image a source supplied, to choose (a `button`; `data-selected` on the one shown). |
| `import` | `[data-cs="game-editor"] [data-part="import"]` | Imports an image file (a `button`). |
| `clear` | `[data-cs="game-editor"] [data-part="clear"]` | Shows no image of that kind (a `button`). |
| `cancel` | `[data-cs="game-editor"] [data-part="cancel"]` | Closes the editor, asking first when there are unsaved changes (a `button`). |
| `save` | `[data-cs="game-editor"] [data-part="save"]` | Saves the Details (a `button`). |

Contains: [`button`](#button), [`text-field`](#text-field), [`checkbox`](#checkbox), [`slider`](#slider)

## `manual-game`

Adds a game by hand from Add game: one on this computer or behind a URI, started where it is, or a title-only record; games that may be the same offer to join or keep apart. Rendered by ManualGameDialog (apps/native/src/flows/manual_game.rs); in the webview, ManualGameDialog (app).

Root: `[data-cs="manual-game"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `target` | `[data-cs="manual-game"] [data-part="target"]` | What it starts: the path or URI field, Browse and the drop area (`data-drop-active` while a file is dragged over). |
| `summary` | `[data-cs="manual-game"] [data-part="summary"]` | The program, arguments, folder or URI it will start. |
| `duplicates` | `[data-cs="manual-game"] [data-part="duplicates"]` | Games that may be the same one. |
| `duplicate` | `[data-cs="manual-game"] [data-part="duplicate"]` | One of them, and why. |
| `link` | `[data-cs="manual-game"] [data-part="link"]` | Adds the target to that game instead (a `button`). |
| `error` | `[data-cs="manual-game"] [data-part="error"]` | Why the target can't be added, or adding failed. |
| `add` | `[data-cs="manual-game"] [data-part="add"]` | Adds the game, or keeps it separate from the duplicates (a `button`). |

State on the root:

- `data-mode`: `target`, `title`

Contains: [`button`](#button), [`text-field`](#text-field)

## `selection-bar`

The bar of a library or collection view in selection mode: how many games are chosen, choosing the loaded ones or all of them, clearing, the bulk editor and Done. Rendered by SelectionBar (apps/native/src/library/selection_bar.rs); in the webview, SelectionBar (app).

Root: `[data-cs="selection-bar"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `count` | `[data-cs="selection-bar"] [data-part="count"]` | How many games are chosen. |
| `select-loaded` | `[data-cs="selection-bar"] [data-part="select-loaded"]` | Chooses the games loaded so far (a `button`). |
| `select-all` | `[data-cs="selection-bar"] [data-part="select-all"]` | Chooses every game the view finds, with how many (a `button`). |
| `clear` | `[data-cs="selection-bar"] [data-part="clear"]` | Chooses none (a `button`). |
| `edit` | `[data-cs="selection-bar"] [data-part="edit"]` | Opens the bulk editor (a `button`). |
| `refresh` | `[data-cs="selection-bar"] [data-part="refresh"]` | Refreshes the chosen games' details (a `button`). |
| `done` | `[data-cs="selection-bar"] [data-part="done"]` | Leaves selection mode (a `button`). |

Contains: [`button`](#button)

## `games-refresh-start`

Starts refreshing the chosen games' details and artwork from the built-in sources: missing details only, or the automatic ones fetched again. Rendered by RefreshDialog (apps/native/src/library/selection_bar.rs); in the webview, RefreshDialog (app).

Root: `[data-cs="games-refresh-start"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `start` | `[data-cs="games-refresh-start"] [data-part="start"]` | Starts the refresh (a `button`). |

Contains: [`button`](#button), [`segmented`](#segmented)

## `games-refresh`

The refresh of chosen games' details in Activity: progress, counts, Cancel, and retrying failed games or continuing a cancelled refresh. Rendered by RefreshCard (apps/native/src/activity/refresh.rs); in the webview, RefreshCard (app).

Root: `[data-cs="games-refresh"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `title` | `[data-cs="games-refresh"] [data-part="title"]` | What the refresh is doing. |
| `mode` | `[data-cs="games-refresh"] [data-part="mode"]` | Missing details only, or automatic details refreshed. |
| `progress` | `[data-cs="games-refresh"] [data-part="progress"]` | How many games are through. |
| `counts` | `[data-cs="games-refresh"] [data-part="counts"]` | Updated, unchanged, not found on IGDB, skipped and failed games. |
| `cancel` | `[data-cs="games-refresh"] [data-part="cancel"]` | Stops after the current batch (a `button`). |
| `retry` | `[data-cs="games-refresh"] [data-part="retry"]` | Refreshes the games that failed again (a `button`). |
| `continue` | `[data-cs="games-refresh"] [data-part="continue"]` | Refreshes the games a cancelled refresh left (a `button`). |

State on the root:

- `data-status`: `running`, `done`, `cancelled`

Contains: [`button`](#button)

## `bulk-editor`

One edit on the chosen games: the fields to change, a preview of what changes in which game, and what saved. Rendered by BulkEditDialog (apps/native/src/library/bulk_edit.rs); in the webview, BulkEditDialog (app).

Root: `[data-cs="bulk-editor"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `fields` | `[data-cs="bulk-editor"] [data-part="fields"]` | The fields, each changed only when ticked. |
| `field` | `[data-cs="bulk-editor"] [data-part="field"]` | One field (`data-field`; `data-enabled` when ticked). |
| `list` | `[data-cs="bulk-editor"] [data-part="list"]` | A list field's values and its add field. |
| `chip` | `[data-cs="bulk-editor"] [data-part="chip"]` | One value of a list. |
| `remove` | `[data-cs="bulk-editor"] [data-part="remove"]` | Removes a value from its list (a `button`). |
| `results` | `[data-cs="bulk-editor"] [data-part="results"]` | The preview or the result. |
| `summary` | `[data-cs="bulk-editor"] [data-part="summary"]` | How many games change, already have it, can't take it or are gone. |
| `change` | `[data-cs="bulk-editor"] [data-part="change"]` | One game and what changes in it. |
| `rejected` | `[data-cs="bulk-editor"] [data-part="rejected"]` | A game that can't take the edit, and why. |
| `footer` | `[data-cs="bulk-editor"] [data-part="footer"]` | Status, Back or Cancel, and Preview, Apply or Done. |
| `status` | `[data-cs="bulk-editor"] [data-part="status"]` | Working, or what went wrong. |
| `cancel` | `[data-cs="bulk-editor"] [data-part="cancel"]` | Back to the fields, or Cancel (a `button`). |
| `apply` | `[data-cs="bulk-editor"] [data-part="apply"]` | Preview, Apply or Done (a `button`). |

State on the root:

- `data-step`: `edit`, `preview`, `done`

Contains: [`button`](#button), [`checkbox`](#checkbox), [`segmented`](#segmented), [`text-field`](#text-field), [`slider`](#slider)

## `game-editor-discard`

Asks before an editor with unsaved changes closes: Keep editing or Discard. Rendered by GameEditor (apps/native/src/game/editor/mod.rs); in the webview, GameEditor (app).

Root: `[data-cs="game-editor-discard"]`

Contains: [`button`](#button)

## `game-actions`

The game editor's Actions tab (#178): a release's launch actions, its own launch first, with the form for the one chosen and what Play starts. Rendered by ActionsTab (apps/native/src/game/editor/actions.rs); in the webview, ActionsTab (app).

Root: `[data-cs="game-actions"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `default` | `[data-cs="game-actions"] [data-part="default"]` | What Play starts: one of the play actions, or Ask each time. |
| `actions` | `[data-cs="game-actions"] [data-part="actions"]` | The release's actions (a listbox). |
| `action` | `[data-cs="game-actions"] [data-part="action"]` | One action; `data-kind` is `play` or `utility`, `data-selected` on the one in the form. |
| `form` | `[data-cs="game-actions"] [data-part="form"]` | The chosen action's fields. |
| `locked` | `[data-cs="game-actions"] [data-part="locked"]` | Says the release's own launch comes from its source and how to change it. |
| `target-kind` | `[data-cs="game-actions"] [data-part="target-kind"]` | What an emulator, web or plugin action starts, which the form doesn't change. |
| `path` | `[data-cs="game-actions"] [data-part="path"]` | A path with Browse…; `data-drop-active` while a dropped file is over it. |
| `list` | `[data-cs="game-actions"] [data-part="list"]` | A list of values, one field each: arguments, program names. |
| `env` | `[data-cs="game-actions"] [data-part="env"]` | The environment variables, a name and a value per row. |
| `remove` | `[data-cs="game-actions"] [data-part="remove"]` | Removes one value or variable (a `button`). |
| `proton` | `[data-cs="game-actions"] [data-part="proton"]` | Which Proton build a Windows program runs with: Automatic (the best installed) or one build. |
| `tracking` | `[data-cs="game-actions"] [data-part="tracking"]` | Which programs are the game, for a play action. |
| `error` | `[data-cs="game-actions"] [data-part="error"]` | Why the last save, order, delete or default change was refused. |
| `delete` | `[data-cs="game-actions"] [data-part="delete"]` | Deletes the action; pressed once more to confirm (a `button`). |

Contains: [`button`](#button), [`select`](#select), [`segmented`](#segmented), [`text-field`](#text-field)

## `game-saves`

The game editor's Saves tab (#238): one release's save locations on this device, automatic or the user's own, and the manifest game it matches. Files and folders dropped on it are added. Rendered by SavesTab (apps/native/src/game/editor/saves.rs); in the webview, SavesTab (app).

Root: `[data-cs="game-saves"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `match` | `[data-cs="game-saves"] [data-part="match"]` | What the locations come from; `data-match` is `unknown`, `matched` or `ambiguous`. |
| `mode` | `[data-cs="game-saves"] [data-part="mode"]` | Says whether the automatic locations or the user's own are in use. |
| `locations` | `[data-cs="game-saves"] [data-part="locations"]` | The save locations (a list). |
| `location` | `[data-cs="game-saves"] [data-part="location"]` | One location; `data-kind` is `file`, `folder` or `registry`, with its file count. |
| `path` | `[data-cs="game-saves"] [data-part="path"]` | A location's path or registry key. |
| `empty` | `[data-cs="game-saves"] [data-part="empty"]` | Shown when the release has no save locations. |
| `warnings` | `[data-cs="game-saves"] [data-part="warnings"]` | What couldn't be resolved on this device. |
| `error` | `[data-cs="game-saves"] [data-part="error"]` | Why the locations couldn't load or the last save was refused. |

State on the root:

- `data-choice`: `automatic`, `chosen`

Contains: [`button`](#button), [`select`](#select), [`text-field`](#text-field)

## `scripts`

One scope's lifecycle scripts (#180), in Settings → Scripts, an emulator profile and the game editor: the scripts, the chosen one's form, its test and the order a launch runs every scope's scripts in. Rendered by ScriptsPanel (apps/native/src/flows/scripts.rs); in the webview, ScriptsPanel (app).

Root: `[data-cs="scripts"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `items` | `[data-cs="scripts"] [data-part="items"]` | The scope's scripts (a listbox). |
| `script` | `[data-cs="scripts"] [data-part="script"]` | One script; `data-enabled` is `false` for one that's off, `data-selected` on the one in the form. |
| `form` | `[data-cs="scripts"] [data-part="form"]` | The chosen script's fields. |
| `path` | `[data-cs="scripts"] [data-part="path"]` | A path with Browse… (the program, the working folder); `data-drop-active` while a dropped file is over it. |
| `list` | `[data-cs="scripts"] [data-part="list"]` | A program's arguments, one field each. |
| `remove` | `[data-cs="scripts"] [data-part="remove"]` | Removes one argument (a `button`). |
| `error` | `[data-cs="scripts"] [data-part="error"]` | Why the last save, order or delete was refused. |
| `delete` | `[data-cs="scripts"] [data-part="delete"]` | Deletes the script; pressed once more to confirm (a `button`). |
| `test` | `[data-cs="scripts"] [data-part="test"]` | Test and Cancel test, and the last test's result. |
| `result` | `[data-cs="scripts"] [data-part="result"]` | A test's outcome; `data-status` is how it ended (`succeeded`, `failed`, `timed_out`, `cancelled`, `not_started`). |
| `output` | `[data-cs="scripts"] [data-part="output"]` | A test's standard output or errors. |
| `context` | `[data-cs="scripts"] [data-part="context"]` | The values the test saw. |
| `order` | `[data-cs="scripts"] [data-part="order"]` | The order a launch runs scripts in, every scope's. |
| `step` | `[data-cs="scripts"] [data-part="step"]` | One step; `data-inherited` when it comes from another scope, `data-enabled` `false` when it's off. |

Contains: [`button`](#button), [`select`](#select), [`segmented`](#segmented), [`switch`](#switch), [`text-field`](#text-field)

## `play-choice`

On a game's page, under Play: which of the release's play actions Play starts, and its tools (#178). Rendered by PlayChoice (app).

Root: `[data-cs="play-choice"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `tools` | `[data-cs="play-choice"] [data-part="tools"]` | The release's tools, one button each. |

Contains: [`button`](#button), [`select`](#select)

## `tool-failed`

Says why a game's tool didn't start. Rendered by ToolRequest (apps/native/src/flows/tool_request.rs); in the webview, ToolRequest (app).

Root: `[data-cs="tool-failed"]`

Contains: [`button`](#button)

## `screenshot-viewer`

A game's screenshots one at a time, filling the window, opened from its page; arrows, a swipe, a pad's left/right or LB/RB step through them. Rendered by ScreenshotViewer (apps/native/src/game/media.rs); in the webview, ScreenshotViewer (app).

Root: `[data-cs="screenshot-viewer"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `title` | `[data-cs="screenshot-viewer"] [data-part="title"]` | The game's title. |
| `counter` | `[data-cs="screenshot-viewer"] [data-part="counter"]` | Which screenshot of how many. |
| `close` | `[data-cs="screenshot-viewer"] [data-part="close"]` | Closes the gallery. |
| `stage` | `[data-cs="screenshot-viewer"] [data-part="stage"]` | The area the screenshot fills; a swipe on it steps. |
| `image` | `[data-cs="screenshot-viewer"] [data-part="image"]` | The screenshot, full size. |
| `prev` | `[data-cs="screenshot-viewer"] [data-part="prev"]` | Steps to the previous screenshot. |
| `next` | `[data-cs="screenshot-viewer"] [data-part="next"]` | Steps to the next screenshot. |
| `strip` | `[data-cs="screenshot-viewer"] [data-part="strip"]` | Every screenshot, small, to jump to one. |
| `thumb` | `[data-cs="screenshot-viewer"] [data-part="thumb"]` | One screenshot in the strip; `data-selected` on the one shown. |

## `videos`

A game's videos (IGDB's trailers) in a row on its page and its Store page; each plays in the video window. Rendered by Videos (apps/native/src/game/media.rs); in the webview, Videos (app).

Root: `[data-cs="videos"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `items` | `[data-cs="videos"] [data-part="items"]` | The row of videos. |
| `video` | `[data-cs="videos"] [data-part="video"]` | One video's button: its thumbnail and name. |
| `thumbnail` | `[data-cs="videos"] [data-part="thumbnail"]` | The video's thumbnail. |
| `play` | `[data-cs="videos"] [data-part="play"]` | The play mark over the thumbnail. |
| `name` | `[data-cs="videos"] [data-part="name"]` | The video's name. |

React Aria interaction: `data-hovered`, `data-focus-visible`, `data-pressed`

## `osk`

The on-screen keyboard a pad types with; it types into the focused field, which keeps focus. Rendered by the on-screen keyboard (apps/native/src/input/keyboard.rs); in the webview, OnScreenKeyboard (app).

Root: `[data-cs="osk"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `preview` | `[data-cs="osk"] [data-part="preview"]` | The field's text as it is typed. |
| `row` | `[data-cs="osk"] [data-part="row"]` | A row of keys. |
| `key` | `[data-cs="osk"] [data-part="key"]` | A key; `data-key` is its character or what it does (`shift`, `layer`, `backspace`, `space`, `left`, `right`, `done`), `data-focused` is the pad's place, `data-selected` an active shift. |

State on the root:

- `data-layer`: `letters`, `symbols`
- `data-shift`: `off`, `once`, `lock`

## `type-search`

Search from a keyboard, RetroArch style: a centred overlay with the text being typed and how many games match, opened by typing anywhere outside a text field or by `/`. Rendered by TypeSearch (apps/native/src/frame/type_search.rs); in the webview, TypeSearch (app).

Root: `[data-cs="type-search"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `label` | `[data-cs="type-search"] [data-part="label"]` | The “Search” label. |
| `input` | `[data-cs="type-search"] [data-part="input"]` | The `<input>` holding the typed text. |
| `count` | `[data-cs="type-search"] [data-part="count"]` | How many games match. |
| `hints` | `[data-cs="type-search"] [data-part="hints"]` | The keys for the results, cancelling and deleting. |

## `folder-browser`

Console mode's file and folder picker, in place of the native dialogs a controller can't drive. A opens a folder or picks a file, B cancels, LB/RB go to the previous and next place, LT/RT page through the folder, Start picks the folder shown. `data-kind` is what it picks. Rendered by FolderBrowser (apps/native/src/flows/folder_browser.rs); in the webview, FolderBrowser (app).

Root: `[data-cs="folder-browser"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `title` | `[data-cs="folder-browser"] [data-part="title"]` | What is being picked. |
| `location` | `[data-cs="folder-browser"] [data-part="location"]` | The folder shown, as a path. |
| `places` | `[data-cs="folder-browser"] [data-part="places"]` | Home, the filesystem root and mounted drives. |
| `place` | `[data-cs="folder-browser"] [data-part="place"]` | One place; the one the folder is in has `data-status="active"`. |
| `error` | `[data-cs="folder-browser"] [data-part="error"]` | Why a folder couldn't be opened. |
| `entries` | `[data-cs="folder-browser"] [data-part="entries"]` | The folder's contents, scrolling. |
| `up` | `[data-cs="folder-browser"] [data-part="up"]` | Goes up one folder. |
| `entry` | `[data-cs="folder-browser"] [data-part="entry"]` | A folder or file; `data-kind` is `folder` or `file`. Files that can't be picked carry `data-disabled`. |
| `icon` | `[data-cs="folder-browser"] [data-part="icon"]` | An entry's icon. |
| `name` | `[data-cs="folder-browser"] [data-part="name"]` | An entry's name. |
| `empty` | `[data-cs="folder-browser"] [data-part="empty"]` | Shown in a folder with nothing in it. |
| `actions` | `[data-cs="folder-browser"] [data-part="actions"]` | Cancel, and in folder mode Select this folder. |
| `select` | `[data-cs="folder-browser"] [data-part="select"]` | Picks the folder shown (a `button`). |

State on the root:

- `data-kind`: `folder`, `file`

Contains: [`button`](#button)

## `store-links`

Links to the stores that sell a game. Rendered by WhereToBuy (apps/native/src/store/game.rs); in the webview, StoreLinks (app).

Root: `[data-cs="store-links"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `store-link` | `[data-cs="store-links"] [data-part="store-link"]` | One store; `data-store` names it. |

## `peer-link`

How another device is reached (#225, #231): an icon and a word. `data-status` is `lan` (on this network), `direct` (over the internet), `relay` (through a relay) or `offline`. Rendered by LinkBadge (apps/native/src/settings/devices.rs); in the webview, LinkBadge (app).

Root: `[data-cs="peer-link"]`

State on the root:

- `data-status`: `lan`, `direct`, `relay`, `offline`

## `paired-device`

A device paired with this one: its icon, name, how it is reached, its last sync, Sync now and (for an admin) Forget. `data-status` is how it is reached. Rendered by DeviceRow in Settings → Devices (apps/native/src/settings/devices.rs); in the webview, DeviceRow in Settings → Devices (app).

Root: `[data-cs="paired-device"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `sync` | `[data-cs="paired-device"] [data-part="sync"]` | Its OS and the last sync, or that it is syncing now. |
| `sync-error` | `[data-cs="paired-device"] [data-part="sync-error"]` | Shown when the last sync didn't reach it. |

State on the root:

- `data-status`: `lan`, `direct`, `relay`, `offline`

Contains: [`peer-link`](#peer-link), [`button`](#button)

## `transfer`

A game copied between two of the user's devices (#233): its title, the other device, how it is reached and its progress. `data-status` is `running`, `waitingDirect` (game files wait for a direct path), `done` or `failed`. Rendered by CopiesAndShares in Settings → Devices (apps/native/src/settings/devices.rs); in the webview, CopiesAndShares in Settings → Devices (app).

Root: `[data-cs="transfer"]`

State on the root:

- `data-status`: `running`, `waitingDirect`, `done`, `failed`

Contains: [`peer-link`](#peer-link)

## `share`

A game this device shared once with someone else (#246, #247): its title and size, Copy link and Stop sharing while it runs (Forget after), what it is doing and who received it. `data-status` is `preparing` (its files are being read; the link works already), `ready`, `failed`, `stopped` or `expired`. Rendered by SharesList (apps/native/src/flows/shares_list.rs); in the webview, SharesList in Settings → Devices → Copies and shares (app).

Root: `[data-cs="share"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `state` | `[data-cs="share"] [data-part="state"]` | What the share is doing: reading its files, until when the link works, or that it stopped. |
| `recipients` | `[data-cs="share"] [data-part="recipients"]` | Who opened the link, each with how far they got. |

State on the root:

- `data-status`: `preparing`, `ready`, `failed`, `stopped`, `expired`

Contains: [`share-recipient`](#share-recipient)

## `share-recipient`

Someone who opened a share's link (#247): their device's name, whether they help others receive it, and a bar of how far they got. `data-status` is `receiving`, `away` (not connected now) or `done`. Rendered by ShareRecipients (apps/native/src/flows/shares_common.rs); in the webview, ShareRecipients (app).

Root: `[data-cs="share-recipient"]`

State on the root:

- `data-status`: `receiving`, `away`, `done`

## `received-share`

A game someone shared with this device (#249): its title and sharer, and the Help others receive it switch. `data-status` is `receiving`, `serving` (it helps the share's other recipients) or `done`. Rendered by SharesList (apps/native/src/flows/shares_list.rs); in the webview, SharesList in Settings → Devices → Copies and shares (app).

Root: `[data-cs="received-share"]`

State on the root:

- `data-status`: `receiving`, `serving`, `done`

## `share-game`

Share with someone… (#247): how long the link works, then the link with Copy (and, in console mode, a QR code), what the share is doing, who received it, and Stop sharing. Rendered by ShareGameDialog (apps/native/src/flows/share_game.rs); in the webview, ShareGameDialog (app).

Root: `[data-cs="share-game"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `link` | `[data-cs="share-game"] [data-part="link"]` | The share link, selectable. |
| `qr` | `[data-cs="share-game"] [data-part="qr"]` | The link as a QR code, in console mode. |
| `state` | `[data-cs="share-game"] [data-part="state"]` | What the share is doing. |
| `recipients` | `[data-cs="share-game"] [data-part="recipients"]` | Who opened the link. |

Contains: [`share-recipient`](#share-recipient)

## `receive-share`

Add game → Receive a shared game… (#247): the link, what its sharer says it shares, Help others receive it, and Receive. Rendered by ReceiveShareDialog (apps/native/src/flows/receive_share.rs); in the webview, ReceiveShareDialog (app).

Root: `[data-cs="receive-share"]`

Contains: [`share-info`](#share-info)

## `share-info`

What a share link shares, as its sharer says (#247, #248): who, what, its size and build, until when the link works; for a save, web game or ROM, where it goes here; for a collection, which of its games this device has. `data-status` is `direct`, or `relayOnly` when only a relay reaches the sharer and files wait for a direct connection. Rendered by ShareInfoDetails (apps/native/src/flows/shares_common.rs); in the webview, ShareInfoDetails (app).

Root: `[data-cs="share-info"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `where` | `[data-cs="share-info"] [data-part="where"]` | Where a save, web game or ROM goes on this device. |
| `games` | `[data-cs="share-info"] [data-part="games"]` | A shared collection's games, each with whether it's here (`data-status`). |

State on the root:

- `data-status`: `direct`, `relayOnly`

## `pair-device`

Pairing over any view (#226, #231): the code both devices show, for a pairing this device or the other one started; or a `caskade://pair` link opened from outside, waiting for an admin. Rendered by PairingHost (apps/native/src/flows/pairing.rs); in the webview, PairingHost (app).

Root: `[data-cs="pair-device"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `state` | `[data-cs="pair-device"] [data-part="state"]` | A code pairing; `data-status` is `waiting`, `paired`, `cancelled` or `failed`. |
| `code` | `[data-cs="pair-device"] [data-part="code"]` | The six-digit code, split in two; larger in console mode. |
| `link` | `[data-cs="pair-device"] [data-part="link"]` | An opened pairing link; `data-status` is `valid` or `expired`. |

Contains: [`button`](#button)

## `account-picker`

Who is playing: the accounts to sign in as, at start and after Switch account, and the PIN pad for a locked one. `data-step` is `accounts` or `pin`. Rendered by AccountPicker (apps/native/src/accounts/picker.rs); in the webview, AccountPicker (app).

Root: `[data-cs="account-picker"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `titlebar` | `[data-cs="account-picker"] [data-part="titlebar"]` | The bar along the top: it drags the window, and holds the window controls. |
| `heading` | `[data-cs="account-picker"] [data-part="heading"]` | The title. |
| `accounts` | `[data-cs="account-picker"] [data-part="accounts"]` | The account tiles. |
| `account` | `[data-cs="account-picker"] [data-part="account"]` | An account's tile; `data-status="locked"` when a PIN locks it. The last account to sign in has `data-focus-default`. |
| `avatar` | `[data-cs="account-picker"] [data-part="avatar"]` | An account's picture (an `avatar`). |
| `name` | `[data-cs="account-picker"] [data-part="name"]` | An account's name. |
| `lock` | `[data-cs="account-picker"] [data-part="lock"]` | The lock on a PIN-locked account's tile. |
| `back` | `[data-cs="account-picker"] [data-part="back"]` | Back from the PIN pad to the accounts. |

State on the root:

- `data-step`: `accounts`, `pin`

Contains: [`avatar`](#avatar), [`pin-pad`](#pin-pad), [`window-controls`](#window-controls), [`hint-bar`](#hint-bar)

## `pin-pad`

PIN entry: a field for keys and a number pad for a controller (A types a digit, B deletes, Start confirms). A wrong PIN shakes the field and clears it; after too many, `locked` counts the wait down. Rendered by PinPad (apps/native/src/flows/pin_pad.rs); in the webview, PinPad (app).

Root: `[data-cs="pin-pad"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `title` | `[data-cs="pin-pad"] [data-part="title"]` | What the PIN is for. |
| `field` | `[data-cs="pin-pad"] [data-part="field"]` | The PIN, as dots. |
| `keys` | `[data-cs="pin-pad"] [data-part="keys"]` | The number pad. |
| `key` | `[data-cs="pin-pad"] [data-part="key"]` | A digit. |
| `delete` | `[data-cs="pin-pad"] [data-part="delete"]` | Deletes the last digit. |
| `submit` | `[data-cs="pin-pad"] [data-part="submit"]` | Confirms the PIN. |
| `message` | `[data-cs="pin-pad"] [data-part="message"]` | Why the PIN was refused, or how long to wait. |

State on the root:

- `data-status`: `ready`, `wrong`, `locked`

## `account-menu`

The head of the menu the topbar's avatar opens: who is signed in. Its actions are the `menu`'s items. Rendered by AccountMenu (apps/native/src/accounts/menu.rs); in the webview, AccountMenu (app).

Root: `[data-cs="account-menu"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `avatar` | `[data-cs="account-menu"] [data-part="avatar"]` | The account's picture (an `avatar`). |
| `name` | `[data-cs="account-menu"] [data-part="name"]` | The account's name. |
| `role` | `[data-cs="account-menu"] [data-part="role"]` | Admin or Standard. |

Contains: [`avatar`](#avatar)

## `account-list`

Settings → Accounts, for admins: every account, with Edit and Delete. Rendered by AccountsSettings (apps/native/src/settings/accounts.rs); in the webview, AccountsSettings (app).

Root: `[data-cs="account-list"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `account` | `[data-cs="account-list"] [data-part="account"]` | One account's row. |
| `avatar` | `[data-cs="account-list"] [data-part="avatar"]` | Its picture (an `avatar`). |
| `name` | `[data-cs="account-list"] [data-part="name"]` | Its name; the signed-in account's says so. |
| `details` | `[data-cs="account-list"] [data-part="details"]` | Its role, whether a PIN locks it, and what it may do. |
| `edit` | `[data-cs="account-list"] [data-part="edit"]` | Opens the `account-editor` (a `button`). |
| `delete` | `[data-cs="account-list"] [data-part="delete"]` | Asks before deleting the account and its collections (a `button`). |

Contains: [`avatar`](#avatar), [`button`](#button)

## `account-editor`

The dialog that adds or changes an account: its name, avatar, role and PIN, and for a standard account what it may change and its age limit. Rendered by AccountEditor (apps/native/src/settings/accounts.rs); in the webview, AccountEditor (app).

Root: `[data-cs="account-editor"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `avatars` | `[data-cs="account-editor"] [data-part="avatars"]` | The avatar choice (an `avatar-picker`) and its label. |
| `access` | `[data-cs="account-editor"] [data-part="access"]` | A standard account's permissions: Install games, one switch per Settings section, the maximum age rating and unrated games. |

Contains: [`text-field`](#text-field), [`avatar-picker`](#avatar-picker), [`setting-row`](#setting-row), [`segmented`](#segmented), [`switch`](#switch), [`select`](#select), [`button`](#button)

## `account-profile`

Settings → My profile: the signed-in account's own name, avatar and PIN. Rendered by ProfileSettings (apps/native/src/settings/accounts.rs); in the webview, ProfileSettings (app).

Root: `[data-cs="account-profile"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `avatars` | `[data-cs="account-profile"] [data-part="avatars"]` | The avatar choice (an `avatar-picker`) and its label. |
| `pin` | `[data-cs="account-profile"] [data-part="pin"]` | The PIN's row: set, change or remove it, after the current PIN. |

Contains: [`text-field`](#text-field), [`avatar-picker`](#avatar-picker), [`setting-row`](#setting-row), [`button`](#button)

## `addon-catalogue`

Settings → Plugins' catalogue of curated plugins and themes: what each means here, searchable and by kind, from the last fetch when offline. Rendered by AddonCatalogue (apps/native/src/settings/addons.rs); in the webview, AddonCatalogue (app).

Root: `[data-cs="addon-catalogue"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `refresh` | `[data-cs="addon-catalogue"] [data-part="refresh"]` | Check for updates: fetches the catalogue again (a `button`). |
| `status` | `[data-cs="addon-catalogue"] [data-part="status"]` | When the catalogue was fetched, offline or why the last check failed. |
| `updates` | `[data-cs="addon-catalogue"] [data-part="updates"]` | How many updates are waiting. |
| `update-preference` | `[data-cs="addon-catalogue"] [data-part="update-preference"]` | Whether to check for updates when Settings opens (a `switch`). |
| `item` | `[data-cs="addon-catalogue"] [data-part="item"]` | One add-on (`data-kind`: `plugin` or `theme`). |
| `state` | `[data-cs="addon-catalogue"] [data-part="state"]` | What the add-on means here (`data-state`: `available`, `installed`, `updateAvailable` or `incompatible`). |
| `details` | `[data-cs="addon-catalogue"] [data-part="details"]` | Opens the add-on's details (a `button`). |

Contains: [`button`](#button), [`switch`](#switch), [`text-field`](#text-field), [`segmented`](#segmented)

## `addon-details`

A catalogue add-on's details: screenshots, description as text, compatibility, the release an install gets, and installing, updating, applying and removing it. Rendered by AddonDetails (apps/native/src/settings/addons.rs); in the webview, AddonDetails (app).

Root: `[data-cs="addon-details"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `screenshots` | `[data-cs="addon-details"] [data-part="screenshots"]` | The catalogue's screenshots. |
| `screenshot` | `[data-cs="addon-details"] [data-part="screenshot"]` | One screenshot, a thumbnail the app re-encoded. |
| `screenshot-missing` | `[data-cs="addon-details"] [data-part="screenshot-missing"]` | In place of a screenshot that couldn't load. |
| `description` | `[data-cs="addon-details"] [data-part="description"]` | The description, always shown as text. |
| `reasons` | `[data-cs="addon-details"] [data-part="reasons"]` | Why the add-on doesn't fit here, or what it lacks. |
| `release` | `[data-cs="addon-details"] [data-part="release"]` | The release an install gets: version, date, OSes, modes and permissions. |
| `state` | `[data-cs="addon-details"] [data-part="state"]` | What the add-on means here (`data-state`: `available`, `installed`, `updateAvailable` or `incompatible`). |
| `progress` | `[data-cs="addon-details"] [data-part="progress"]` | Downloading, checking, installing or starting. |
| `outcome` | `[data-cs="addon-details"] [data-part="outcome"]` | What the last install did (`data-outcome`: `installed` or `failed`), or a release refused before. |
| `remove-confirm` | `[data-cs="addon-details"] [data-part="remove-confirm"]` | Asks before removing; for a plugin, whether to keep its settings and data. |
| `remove-confirmed` | `[data-cs="addon-details"] [data-part="remove-confirmed"]` | Removes it (a `button`). |
| `actions` | `[data-cs="addon-details"] [data-part="actions"]` | The add-on's buttons. |
| `remove` | `[data-cs="addon-details"] [data-part="remove"]` | Uninstall… (a `button`). |
| `skip` | `[data-cs="addon-details"] [data-part="skip"]` | Skips this update, or offers it again (a `button`). |
| `apply` | `[data-cs="addon-details"] [data-part="apply"]` | Makes an installed theme the one in use (a `button`). |
| `install` | `[data-cs="addon-details"] [data-part="install"]` | Install, Update or Try again (a `button`). |

State on the root:

- `data-kind`: `plugin`, `theme`
- `data-state`: `available`, `installed`, `updateAvailable`, `incompatible`

Contains: [`button`](#button), [`checkbox`](#checkbox)

## `plugin-ui`

What a plugin draws in a slot or view (plugin API 0.2): Caskade's components as the plugin's backend asked for them. Rendered by PluginTree (apps/native/src/plugins/tree.rs); in the webview, PluginTree (app).

Root: `[data-cs="plugin-ui"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `text` | `[data-cs="plugin-ui"] [data-part="text"]` | Plain text. |
| `heading` | `[data-cs="plugin-ui"] [data-part="heading"]` | A heading (`h2`, or `h3` at level 3). |
| `paragraph` | `[data-cs="plugin-ui"] [data-part="paragraph"]` | A paragraph. |
| `note` | `[data-cs="plugin-ui"] [data-part="note"]` | Secondary text, muted. |
| `alert` | `[data-cs="plugin-ui"] [data-part="alert"]` | Something that went wrong, announced. |
| `stack` | `[data-cs="plugin-ui"] [data-part="stack"]` | Children in a column or a row (`data-direction`). |
| `section` | `[data-cs="plugin-ui"] [data-part="section"]` | A labelled part that pad focus moves between. |
| `stats` | `[data-cs="plugin-ui"] [data-part="stats"]` | Labelled values (a description list). |
| `tile` | `[data-cs="plugin-ui"] [data-part="tile"]` | One large value with what it is. |
| `value` | `[data-cs="plugin-ui"] [data-part="value"]` | A tile's value. |
| `label` | `[data-cs="plugin-ui"] [data-part="label"]` | A tile's label. |
| `bars` | `[data-cs="plugin-ui"] [data-part="bars"]` | A bar chart, with each value as text. |
| `bar` | `[data-cs="plugin-ui"] [data-part="bar"]` | One bar. |
| `table` | `[data-cs="plugin-ui"] [data-part="table"]` | A table. |
| `list` | `[data-cs="plugin-ui"] [data-part="list"]` | A list. |
| `badge` | `[data-cs="plugin-ui"] [data-part="badge"]` | A short status label (`data-tone`). |
| `progress` | `[data-cs="plugin-ui"] [data-part="progress"]` | A progress bar. |
| `button` | `[data-cs="plugin-ui"] [data-part="button"]` | A button that tells the plugin (a `button`). |
| `segmented` | `[data-cs="plugin-ui"] [data-part="segmented"]` | One of a few options that tells the plugin (a `segmented`). |

Contains: [`button`](#button), [`segmented`](#segmented)

## `plugin-error`

In place of a plugin's slot or view that failed to draw; it holds focus, so a pad can move on. Rendered by Placeholder (apps/native/src/plugins/mod.rs); in the webview, PluginBoundary (app).

Root: `[data-cs="plugin-error"]`

## `achievements`

A game page's achievements: one store account's set at a time, its counts and how its last fetch went, and every achievement with its state, progress, unlock time, rarity and points where the service gives them. Hidden achievements stay concealed until unlocked or revealed. Rendered by GameAchievements (apps/native/src/game/achievements.rs); in the webview, GameAchievements (app).

Root: `[data-cs="achievements"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `refresh` | `[data-cs="achievements"] [data-part="refresh"]` | Refresh achievements: fetches the game's sets now (a `button`). |
| `cancel` | `[data-cs="achievements"] [data-part="cancel"]` | Stops a refresh under way (a `button`). |
| `sets` | `[data-cs="achievements"] [data-part="sets"]` | Which store account's set is shown, when there are several (a `segmented`). |
| `account` | `[data-cs="achievements"] [data-part="account"]` | The set's service and account, and when it was last fetched. |
| `stale` | `[data-cs="achievements"] [data-part="stale"]` | The last refresh failed: what is shown is the last answer, and why. |
| `wait` | `[data-cs="achievements"] [data-part="wait"]` | The service asked Caskade to wait, and until when. |
| `notice` | `[data-cs="achievements"] [data-part="notice"]` | Not fetched yet, no achievements, not offered, private, or signed out, with what to do. |
| `reconnect` | `[data-cs="achievements"] [data-part="reconnect"]` | Opens Settings → Store accounts to sign in again (a `button`). |
| `summary` | `[data-cs="achievements"] [data-part="summary"]` | Unlocked of total, hardcore unlocks and points, with a meter. |
| `filter` | `[data-cs="achievements"] [data-part="filter"]` | All, unlocked or locked (a `segmented`). |
| `reveal-all` | `[data-cs="achievements"] [data-part="reveal-all"]` | Shows every hidden achievement (a `switch`). |
| `list` | `[data-cs="achievements"] [data-part="list"]` | The achievements, in the service's order. |
| `achievement` | `[data-cs="achievements"] [data-part="achievement"]` | One achievement (`data-state`: `unlocked` or `locked`; `data-concealed` while a hidden one is concealed; `data-hardcore` when unlocked in hardcore). |
| `icon` | `[data-cs="achievements"] [data-part="icon"]` | Its icon from the service, or a drawn trophy, lock or concealed mark. |
| `details` | `[data-cs="achievements"] [data-part="details"]` | Unlock time, hardcore, rarity and points. |
| `hardcore` | `[data-cs="achievements"] [data-part="hardcore"]` | Its hardcore unlock, or that it was unlocked in softcore only. |
| `reveal` | `[data-cs="achievements"] [data-part="reveal"]` | Shows a hidden achievement's name and description (a `button`). |

State on the root:

- `data-state`: `loading`, `error`, `not_fetched`, `ready`, `no_achievements`, `unsupported`, `private`, `sign_in_needed`

Contains: [`button`](#button), [`segmented`](#segmented), [`switch`](#switch)

## `achievement-summary`

A game's achievement counts in its hero, across its sets; pressing it moves to the achievements. Rendered by AchievementSummary (apps/native/src/game/achievements.rs); in the webview, AchievementSummary (app).

Root: `[data-cs="achievement-summary"]`

## `romm`

Settings → RomM servers: the server list, the selected server's editor, downloads and the cache. Rendered by RommPanel (apps/native/src/settings/emulation/romm.rs); in the webview, RommPanel (app).

Root: `[data-cs="romm"]`

Container: its content comes from the caller, which may add parts of its own.

Contains: [`settings-section`](#settings-section), [`romm-servers`](#romm-servers)

## `romm-servers`

The RomM server list: each server's state, games, last sync and its action (Sync now, or Sign in when this device can't sign in). Rendered by RommPanel (apps/native/src/settings/emulation/romm.rs); in the webview, RommPanel (app).

Root: `[data-cs="romm-servers"]`

| Part | Selector | What it is |
| --- | --- | --- |
| `romm-server` | `[data-cs="romm-servers"] [data-part="romm-server"]` | A server's row (`data-status`: `untested`, `connected`, `offline`, `error` or `sign_in_needed`; `data-selected` while its editor is open). |
| `sign-in` | `[data-cs="romm-servers"] [data-part="sign-in"]` | Sign in or Enter API token on a server this device can't sign in to: opens its editor at the password or token (a `button`). |
| `sign-in-reason` | `[data-cs="romm-servers"] [data-part="sign-in-reason"]` | Why a server needs signing in on this device: sign-ins stay on each device, or the server refused the saved one. |

Contains: [`button`](#button), [`status-dot`](#status-dot)
