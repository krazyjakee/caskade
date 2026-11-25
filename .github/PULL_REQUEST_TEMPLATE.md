## Summary

<!-- Explain what this pull request changes and why. -->

## Linked issue

<!-- e.g. Closes #123 or Refs #123 -->

## Verification

<!-- Note which commands and manual checks you ran (see AGENTS.md and justfile). -->

- [ ] `just test-app`
- [ ] `just test-native` (if native UI changed)
- [ ] `cargo +1.100.0 clippy --workspace --all-targets -- -D warnings`
- [ ] `pnpm typecheck` and `pnpm lint` (if JS/CSS/token tooling changed)
- [ ] `just i18n-check` (if visible strings or translations changed)

## Checklist

- [ ] Updated `docs/PRODUCT-SPEC.md`, `docs/TECH-SPEC.md`, or `docs/DESIGN-GUIDELINES.md` if behavior, architecture, or UI contracts changed
- [ ] Rebuilt the native stylesheet (`just native-css`) if classes in `apps/native/src` or files in `apps/native/styles` changed
- [ ] Extracted and updated translations (`just i18n`) if visible strings (`t!` / `tr!`) changed
- [ ] Regenerated any affected generated files (design tokens, anatomy docs, legal notices, plugin/backdrop schemas — see `AGENTS.md`)
- [ ] Formatted only touched files (`rustfmt <file>` / `biome format <file>`)
