# Console mode: the native checklist (#153)

CI proves console mode with a synthetic pad (`apps/desktop/src-tauri/tests/console_e2e.rs`,
under Xvfb). These checks need real hardware and stay deferred: they are W10, H2
and H3 in [the manual checks](manual-checks.md). Record each result with the
commit you tested. Issue numbers on this page are archived Gitea issues, which
old commits cite.

Build: `pnpm tauri build --debug` (the AppImage or deb), or `just dev` on the device.

## Steam Deck game mode (gamescope)

Install the AppImage on the Deck. In desktop mode, add it to Steam as a non-Steam
game, then switch to game mode and launch it from the library.

1. **Starts in console mode.** It opens fullscreen in console mode, with no
   `--console` flag: Settings → General → Start in is **Automatic**, and
   gamescope is detected. Set **Start in** to **Desktop**, relaunch, and check
   it now opens on the desktop shell.
2. **Steam Input on** (the default for non-Steam games). The d-pad and left stick
   move focus. A selects, B goes back, X opens a card's details, and Y goes to
   the search. LB/RB page Home's shelves and switch Settings tabs, and LT/RT
   scroll a long list by a screen. The hint bar shows Xbox glyphs.
3. **Steam Input off** (the controller settings for Caskade → Disable Steam
   Input). Repeat 2. The pad is read directly, and the glyphs follow the
   Deck's own pad.
4. **Guide.** Launch a game from Caskade. Press the Steam button (or Select +
   Start, the guide chord) during play. Caskade comes to the front with the
   quick menu. **Return to game** puts the game back in front. Do it again and
   choose **Stop game**: the game ends, and its page shows the new session's
   playtime. Note whether gamescope or Steam itself takes the Steam button
   first.
5. **Fullscreen.** Console mode fills the screen with no window chrome. Press
   **View** to go to desktop mode, then back. In desktop mode, the gamepad
   button next to maximize returns to console mode.
6. **On-screen keyboard.** Press Y to focus the search, and Steam + X opens
   the keyboard. Type, press down for the suggestions, then A. Settings →
   Plugins in console mode takes a typed folder path the same way, since there
   is no folder picker.
7. **Suspend and resume.** Suspend the Deck from the power menu with Caskade in
   front, then resume. The pad still drives it, focus is where it was, and the
   clock in the hint bar is current.

## Windows: a real XInput pad

On Windows 10 or 11 with an Xbox controller (USB or Bluetooth):

1. Start Caskade and press **View** on the pad: it switches to console mode,
   fullscreen. The glyphs are Xbox's.
2. Repeat Steam Deck step 2 without Steam.
3. Guide: launch a game, then press the Xbox button (or Select + Start, which
   a VM has shown working). Caskade comes to the front, or flashes in the
   taskbar if Windows refuses focus. Note which, and whether the Game Bar takes
   the button first. Check Return to game and Stop
   game as in Steam Deck step 4.
4. Close the window while in console mode, then reopen Caskade. The desktop
   window has its old size and position (window-state), and with **Start in**
   set to **Automatic** it opens in console mode, the mode it was left in.

## Linux: a DualSense over Bluetooth

On a desktop Linux session (GNOME or KDE), pair the controller over Bluetooth:

1. `caskade --console` opens in console mode. The hint bar shows PlayStation
   glyphs (✕ ○ □ △, L1/R1).
2. Repeat Steam Deck step 2. ✕ selects and ○ goes back.
3. The PS button summons Caskade over a running game (Steam Deck step 4). On
   Wayland, if the compositor refuses focus, Caskade asks for attention
   instead: note which.
4. Disconnect the controller with Caskade in front, then reconnect it. Input
   resumes without a restart.

## What each check covers

The commits that implement what each check exercises. Test at or after the
newest one listed.

| Check | Commits |
|---|---|
| Starting in console mode, gamescope detection, Start in | `b6f3f3c` (with `3dee3f5`, which carried `commands/console.rs`) |
| Pad navigation, Steam Input on and off, hot plug | `dae8c00` (#146, gilrs input), `a687963` (#147, focus engine) |
| Hint bar glyphs per pad family, LB/RB and LT/RT paging | `b6f3f3c` (glyphs from `3dee3f5`) |
| Guide, the quick menu, Return to game, Stop game | `62b0387` (#149) |
| Fullscreen, View and the console button, window state on exit | `b6f3f3c` |
| Search by Y with the OS keyboard, typed plugin folder | `b6f3f3c` |
| Suspend and resume (input service, the hint bar's clock) | `dae8c00`, `b6f3f3c` |
| Frame rate on NVIDIA under X11 | `bdb6460` |
