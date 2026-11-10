# Design guidelines

How Caskade should look, behave and read. These apply to the app, its components and the plugins and themes built for it. The mechanics behind them (tokens, anatomy, layers) are in the [theming contract](theming/README.md); what Caskade does is in the [product spec](PRODUCT-SPEC.md).

## 1. Character

Caskade is a games console for your whole collection. It should feel **fast, calm and confident**, with the games' own art doing most of the talking.

- **Art first.** Covers, heroes and screenshots carry the colour and the mood. The interface around them stays dark, quiet and out of the way.
- **One press.** The thing people came to do, play a game, is never more than one press from wherever the game is shown.
- **Calm by default.** Background work is quiet: no toasts for things nobody asked about, no spinners for things that can wait. Problems surface once, where they matter.
- **Honest state.** Show what is true: installed, owned, running elsewhere, offline, waiting for an answer. Never pretend something worked.
- **Desk and couch.** Every screen works with a mouse, a keyboard and a pad, in desktop mode and console mode. If it doesn't work on a pad, it isn't done.

The built-in theme is dark with restrained sci-fi accents: a faint backdrop grid, HUD corner brackets on heroes and panels, scanlines on the hero, a glow and sweep when a card is hovered or focused, a rail on the active navigation item. These are **theme material**, kept in `themes/default/theme.css`, not built into components, so other themes (Paper, Ember) can drop them entirely.

## 2. Layout

- **The shell**: a sidebar (Home, Library with its filters beneath it, Store, Collections, then Settings), a topbar that is also the window's title bar (search, activity bell, account, window controls), the view, and a status bar. Console mode swaps in its own sidebar, topbar and a hint bar along the bottom.
- **Views** open with what matters most: a hero for the game to continue on Home, the game's hero and Play on its page, the grid in the Library.
- **Shelves** are horizontal rows of cards. **Grids** are virtualised and keep their place when you come back.
- **The quick look** (desktop mode) opens beside the view when a game is chosen, so browsing doesn't lose its place. View more opens the page. Console mode goes straight to the page.
- **Settings** are grouped tabs (App, Library, Play, Devices, People, System), each made of `SettingsSection`s and `SettingRow`s: a label and description on the left, the control on the right. Every setting is findable through the settings search.
- **Dialogs** are for decisions. Editors that fit in place open in place.
- View state lives in the URL (filters, search, tabs, sections), so Back, links and restarts return to the same place.

## 3. Visual language

Style only through the `--cs-*` tokens, through their Tailwind utilities (`bg-surface-raised`, `text-fg-muted`, `rounded-card`, …). Never use raw colours, sizes or durations in the app or in components: a theme can't reach them, and the Appearance settings can't adjust them.

- **Surfaces**: `surface-base` (the background), `surface-raised` (panels and cards), `surface-overlay` (menus, popovers, inputs), `surface-hover`. Borders are hairlines (`border`) or control outlines (`border-strong`).
- **Text**: `fg` for primary text, `fg-muted` for secondary, `fg-subtle` for hints and captions.
- **Accent**: one accent for primary actions, selection and focus. The user can pick their own accent, so never rely on its exact hue. Caskade keeps it at 3:1 contrast against the surfaces.
- **Status colours** mean the same everywhere: installed, available, emulated and missing, plus warning and danger. Status is never shown by colour alone: pair it with a label or an icon.
- **Type**: Oxanium for display (headings, numbers, HUD labels), Inter for body text, JetBrains Mono for data, paths and technical labels. All are bundled; the app loads no web fonts.
- **Icons**: lucide line icons, at the sizes the components set.
- **Shape**: rounded cards and controls from the `radius.*` tokens. `chamfer` cuts corners for emphasis. A glow on a chamfered element goes on a wrapper with `filter: drop-shadow`, because the clip also cuts off `box-shadow`.
- **Effects** (blur, glow, grid, scanlines) come from `effect.*` tokens so Reduce effects and slow GPUs can turn them off. They are decoration: the interface must read the same without them.

## 4. Motion

- Durations come from `--cs-motion-*` tokens. Looping animations set `animation-play-state: var(--cs-motion-ambient)`.
- Loops that run for as long as background work or the app does move in steps: the busy spinner (`animate-worker`) turns in `steps(8)`, the backdrop pans in steps. The native UI draws a frame for each step, so a continuous loop costs a share of a core for as long as it runs (#82).
- Reduced motion (the system setting or Appearance) zeroes durations and pauses loops through token sets, so every new animation must use the tokens to inherit that.
- When the webview paints on the CPU (`data-paint="cpu"`), ambient loops pause by default. Never make anything depend on an animation running.
- Lists enter with `animate-enter`, staggered by `--i`. Motion explains a change (something arrived, moved or opened); it never decorates a static screen.

## 5. Input and focus

- **One focus look** for keyboard and pad: the `focus.ring.*` tokens, an outline that never moves layout. In console mode it sits inside the element so a scrolling shelf can't clip it. Focus shows on `:focus-visible` / `data-focus-visible` only; the native UI sets only `data-focus-visible`, so its styles select that.
- **Console mode buttons mean the same on every view**: A selects, B goes back or closes, X opens a card's quick look (X again plays), Y searches, Start opens the focused item's menu, LB/RB switch the view's main tabs, LT/RT page the list focus is in. Guide brings Caskade back from a game. The hint bar shows each view's bindings.
- **Spatial navigation** stays within the current area while it has a target that way, prefers the row or column focus is already in, and treats a settings row as one stop.
- Each view gives a pad its first stop (`data-focus-default`). A dialog moves focus to its default control, and back to where it was when it closes. In the native UI a dialog or popover closes on B through its own `back` binding.
- **Text entry** on a pad goes through the on-screen keyboard. Typing anywhere outside a field starts a search.
- **Files and folders**: every control that opens a picker is also a drop zone, and pickers go through `usePathPicker`, which shows the in-app folder browser in console mode.
- **Menus**: every item that has a context menu opens it from a right click, the context-menu key, Shift+F10, a pad's Start or an actions button. A game has one menu, the same everywhere.
- Mouse-only gestures (hover, drag) always have another way in.

## 6. Components and anatomy

- Build from `@caskade/ui`: form controls on React Aria Components, `SettingsSection`/`SettingRow`, `GameCard`, `Hero`, `Shelf`, menus, `ErrorNotice`. Add a component there when two places need it.
- Style interaction state with React Aria's data attributes (`data-hovered`, `data-selected`, `data-focus-visible`), not `:hover` or `:checked`.
- Give every component a `data-cs` root and `data-part`s, and domain state as `data-status` (and similar). Add them to the anatomy catalogue (`packages/ui/src/anatomy.ts`): they are public API for themes. A native (RSX) component declares the same entry in `apps/native/src/anatomy.rs`, word for word apart from the component it names; until the native UI is the default, a test holds the two catalogues together. Class names are not, and CSS never selects on classes.
- A control inside a `SettingRow` is labelled by its row. A second control in the same row needs its own `aria-label`.

## 7. Writing

Write the way you would explain it to someone sitting next to you. Plain words, short sentences, no jargon a player wouldn't use.

- **British English** ("favourite", "colour", "licence").
- **Sentence case** for buttons, labels, headings and menu items ("Back up now", "Add to collection…"). The Library's filters and Home's shelves are names and keep Title Case ("Recently Played", "Ready to Install").
- **Name things by what they do for the player**, not by the technology: "streaming host", "the device key", "Proton". Never show the names of programs Caskade drives invisibly (Sunshine, Moonlight), internal ids, namespaces, type names or full URLs.
- **Say what will happen** before it happens, and what happened after. A confirmation names the consequence ("Removing the server deletes the ROMs downloaded from it").
- **Don't ask twice** for the same thing, and don't ask at all when there is a safe default.

### 7.1 Verbs

One verb for one meaning, so a button says what it does before it is pressed.

| Verb | Meaning | Examples |
| --- | --- | --- |
| **Forget** | Ends a pairing or link with another device, host or person. Nothing on either side is deleted. | A paired device, a streaming host, a finished share |
| **Remove** | Takes something Caskade manages or lists out of Caskade. Its files stay. | An emulator, a ROM scanner, a storage location, a store account, a PIN |
| **Delete** | Destroys something for good. | A script, a local account |
| **Uninstall** | Takes a game or an add-on off this device, files included. | A game, a plugin, a theme |
| **Discard** | Drops edits that were never saved. | Unsaved changes, a scan review |
| **Disconnect** | Signs out of a store account and keeps its library. | Steam, GOG, Epic |
| **Revert** | Returns an editor that stays open to its last saved version. | |
| **Clear** | Empties a cache, which refills by itself. | |
| **Skip this version** | Declines one update. Skip on its own is a step in a flow. | |

Where Remove takes data with it, the confirmation says so; the verb doesn't change.

### 7.2 Ellipses

A button or menu item ends with an ellipsis when it asks something before it acts: it opens a dialog or a picker, or puts a question in its row. Actions that happen at once, links to other pages and the button that answers the question have none: Remove… opens the dialog, Remove confirms it. An editor that opens in place isn't a dialog. `aria-label`s keep the plain words without the ellipsis.

### 7.3 Errors

- Say **what failed**, the **resource** (file name, folder, host, game; never a full URL or a secret), the **cause** when known, and the **effect**: "The game's folder “/games/Celeste” is missing. It may have been moved, deleted or on a drive that isn't connected."
- Tell a confirmed cause from a possible one. Say what Caskade is already doing about it, and the next step if that fails. Never send people to the logs for the basic explanation.
- No "Error:", "failed to", "invalid", "something went wrong" or raw debug output.
- **One notice per failure**: inline next to the control, a block in place of a view, a job's recovery in Activity, or a notification for background work. Never repeated toasts.

### 7.4 Translation

Every visible string goes through the translation layer (Lingui in the webview, `tr!` in Rust). Name placeholders by what fills them (`{title}`, not `{0}`). Add a translator comment when the English alone is ambiguous, and a context only when the same English means different things. Leave room: translations run longer than English.

## 8. Accessibility

- Everything works with a keyboard alone and with a pad alone.
- Every control has an accessible name; icon-only buttons get an `aria-label`.
- Text and controls meet contrast against their surfaces; the user's accent is corrected to 3:1, and forced colours draw focus in `Highlight`.
- Respect reduced motion and reduced transparency, the UI scale (0.8–1.5) and compact density. Layouts must hold at the largest scale.
- Never convey state by colour or animation alone.

## 9. Speed is design

- The grid never decodes full-size art: thumbnails come from the cache.
- Prefer flat fills to blur. WebKitGTK on Linux is the slowest webview Caskade runs on; design for it.
- Show cached content immediately and refresh it quietly. A view never waits on the network to show what Caskade already knows.
- Long work becomes a job in Activity with progress and cancel, never a frozen screen.
