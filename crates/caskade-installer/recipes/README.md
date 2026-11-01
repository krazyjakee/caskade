# Installer recipes

Recipes describe optional overrides using the existing typed plan steps. They
are TOML data, never evaluated as shell or Rust code while loading or planning.
The plan engine validates transformed candidates and applies the caller's
original host and execution policy afterwards.

```toml
schema_version = 1
id = "my-game"
strategy = "extract"
disable_alias = true

[match]
gog_id = "1234567890"
# app_id = "InstallerAppId"
# sha256 = "64 hexadecimal characters"

[preset_args]
language = "english"
no_shortcuts = true

[[add_steps]]
type = "write_file"
[add_steps.value]
path = "recipe-applied.txt"
contents = [111, 107, 10] # UTF-8 bytes: "ok\n"
```

At least one exact selector is required. Multiple selectors are ANDed; hash
comparison and installer AppId comparison ignore ASCII case. GOG IDs compare
exactly. A strategy filters the existing proposals: it does not invent an
extraction capability when no handler provides one. Added steps use the same
`Step` wire representation as manifests and plans. Process steps remain subject
to execution policy. All file destinations are validated as game-relative paths.

`RecipeSet::load(user_directory)` loads the compiled-in files and then optional
regular `*.toml` files from that directory. A user recipe replaces a builtin with
the same ID. Distinct matching IDs merge in sorted ID order; conflicting strategy,
preset or alias values return a clear error. Directory loading is atomic. Recipe
counts, bytes, steps and metadata are bounded; symlink recipe files are rejected.
`json_schema()` exposes the schemars schema used for actual JSON Schema validation
after TOML decoding. HTTP/file schema reference resolution is disabled.

Bind input identity at the I/O boundary with `recipes.bind_input(&node, &path)`;
it computes SHA-256 only when the loaded set needs a hash. GOG/installer IDs come
from supplied artifact facts or an explicit `InputIdentity`. Call
`resolve_artifact(&node)` to inspect matching IDs and `disable_alias` before alias
orchestration. Then pass the set to
`PlanEngine::with_transform(Box::new(recipes))`. Planning itself performs no I/O.

The builtin fixture recipe is pinned to the committed `inno60-basic` setup hash.
Its end-to-end test verifies ordinary Windows planning chooses `RunSilent`, the
recipe chooses `Extract`, and the executor extracts the genuine fixture and writes
its marker without starting any process.
