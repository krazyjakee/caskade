# Crashy

A test-only plugin for #134 that misbehaves on purpose, to show one broken plugin doesn't take Caskade down:

- its view, home section and game-details panel throw while rendering, and the shell shows placeholders;
- its backend traps on every `game.exited`;
- its `launch.before` interceptor never returns. Each step is time-boxed and skipped, and after three failures in a row the circuit breaker turns the plugin off with a notice;
- its `grow` request allocates past the memory limit, which fails just that call.

Don't install it except to watch this happen.
