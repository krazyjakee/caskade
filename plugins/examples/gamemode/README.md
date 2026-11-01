# GameMode

A backend-only plugin for Linux. A `launch.before` interceptor runs games through [Feral GameMode](https://github.com/FeralInteractive/gamemode)'s `gamemoderun` (`gamemoderun <game> <args…>`). `gamemoderun` execs the game, so Caskade still tracks the right process.

Permissions:
- `process: true`, because changing what runs needs it;
- read-only `/usr/bin`, `/usr/games` and `/usr/local/bin`, to see whether `gamemoderun` is installed. Without it, launches are left alone.

Settings:
- **Run games with GameMode** turns it off.
- **gamemoderun path** names a specific binary. It is used without checking, since the plugin can't see other folders.
