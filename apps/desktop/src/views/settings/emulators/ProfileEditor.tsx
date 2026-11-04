import {
  Button,
  Checkbox,
  SegmentedControl,
  Select,
  SettingsSection,
  TextField,
} from "@caskade/ui";
import { msg } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { useQuery } from "@tanstack/react-query";
import { Plus, X } from "lucide-react";
import { useEffect, useState } from "react";
import {
  argvText,
  type ConfiguredEmulator,
  type EmulationSnapshot,
  type ProfileEdit,
} from "../../../api/emulation";
import { ScriptsPanel } from "../../../components/scripts/ScriptsPanel";
import { UnsavedHint, useUnsavedChanges } from "../unsavedChanges";
import { PathField, PreviewTokens } from "./EmulationFields";
import { useEmulationApi, useEmulationWrite } from "./EmulationProvider";

const argumentModes = [
  {
    id: "default",
    label: msg({
      message: "Default",
      context: "emulator arguments",
      comment: "Use the specification's arguments",
    }),
  },
  {
    id: "replace",
    label: msg({
      message: "Replace",
      context: "emulator arguments",
      comment: "Replace the specification's arguments",
    }),
  },
  {
    id: "append",
    label: msg({
      message: "Append",
      context: "emulator arguments",
      comment: "Add arguments after the specification's",
    }),
  },
] as const;
const trackingModes = [
  { id: "default", label: msg({ message: "Default", context: "playtime tracking" }) },
  { id: "process_tree", label: msg({ message: "Process tree", context: "playtime tracking" }) },
  {
    id: "original_process",
    label: msg({ message: "Original process only", context: "playtime tracking" }),
  },
  { id: "folder", label: msg({ message: "Any process in folder", context: "playtime tracking" }) },
  { id: "process_name", label: msg({ message: "Process name", context: "playtime tracking" }) },
] as const;
export function ProfileEditor({
  emulator,
  initial,
  snapshot,
  close,
}: {
  emulator: ConfiguredEmulator;
  initial: ProfileEdit;
  snapshot: EmulationSnapshot;
  close: () => void;
}) {
  const { t } = useLingui();
  const unknownProfile = t`Unknown built-in profile`;
  const noExtensions = t({ message: "None", context: "file extensions" });
  const noArgs = t({ message: "(none)", comment: "No arguments" });
  const api = useEmulationApi();
  const save = useEmulationWrite();
  const [draft, setDraft] = useState(() => structuredClone(initial));
  const [debounced, setDebounced] = useState(draft);
  const [start] = useState(() => JSON.stringify(initial));
  const dirty = JSON.stringify(draft) !== start;
  const submit = async () => {
    await save.mutateAsync((api) => api.saveProfile(emulator.id, draft));
    close();
    return true;
  };
  const profile = draft.profile;
  const [extensions, setExtensions] = useState(
    profile.type === "custom" ? profile.extensions.join(", ") : "",
  );
  const title = profile.type === "custom" ? t`Custom profile` : profile.definitionProfile;
  // The profile waits for Save profile (#43): leaving the section asks first.
  useUnsavedChanges(dirty, {
    label: `${emulator.name}: ${title}`,
    save: submit,
    discard: () => {
      const reset = structuredClone(initial);
      setDraft(reset);
      setExtensions(reset.profile.type === "custom" ? reset.profile.extensions.join(", ") : "");
    },
  });
  useEffect(() => {
    const timer = setTimeout(() => setDebounced(draft), 200);
    return () => clearTimeout(timer);
  }, [draft]);
  const preview = useQuery({
    queryKey: ["emulation", "preview", emulator.id, debounced],
    queryFn: () => api.preview(emulator.id, debounced),
    retry: false,
  });
  const current = emulator.profiles.find((row) => row.config.id === profile.id);
  // Cleared once the user picks how to resolve it; saving then records the reviewed args.
  const drift =
    current?.resolved.drift &&
    !(
      profile.type === "built_in" &&
      profile.args.type === "replace" &&
      profile.args.replaced === current.resolved.drift.definitionArgs
    )
      ? current.resolved.drift
      : null;
  const definition = snapshot.definitions
    .find((row) => row.id === emulator.definition)
    ?.profiles.find((row) => profile.type === "built_in" && row.name === profile.definitionProfile);
  const variables = [
    ...snapshot.variables,
    ...(snapshot.definitions.find((row) => row.id === emulator.definition)?.vars ?? [])
      .filter((name) => !snapshot.variables.some((row) => row.name === name))
      .map((name) => ({ name, description: t`Set by Caskade for this emulator` })),
  ];
  return (
    <SettingsSection
      title={title}
      description={t`Caskade checks the profile when you save. Nothing here starts the emulator.`}
      aside={
        <Button
          variant="ghost"
          size="sm"
          icon={X}
          aria-label={t`Close profile editor`}
          onPress={close}
          isDisabled={save.isPending}
        />
      }
    >
      <div data-cs="emulation-profile-editor" className="mt-4 space-y-4">
        {profile.type === "custom" ? (
          <>
            <div className="grid gap-4 md:grid-cols-2">
              <TextField
                label={t`Profile name`}
                value={profile.name}
                onChange={(name) => setDraft((old) => ({ ...old, profile: { ...profile, name } }))}
              />
              <PathField
                label={t({ message: "Executable", comment: "An emulator profile's program" })}
                kind="file"
                value={profile.executable ?? ""}
                onChange={(executable) =>
                  setDraft((old) => ({
                    ...old,
                    profile: { ...profile, executable: executable || null },
                  }))
                }
              />
              <TextField
                label={t`Working directory`}
                mono
                value={profile.workingDir ?? ""}
                onChange={(workingDir) =>
                  setDraft((old) => ({
                    ...old,
                    profile: { ...profile, workingDir: workingDir || null },
                  }))
                }
              />
              <TextField
                label={t`File types`}
                description={t({
                  message:
                    "Comma-separated, without dots. <none> matches a file without an extension.",
                  comment: "<none> is typed as is; don't translate it",
                })}
                value={extensions}
                onChange={(value) => {
                  setExtensions(value);
                  setDraft((old) => ({
                    ...old,
                    profile: {
                      ...profile,
                      extensions: [
                        ...new Set(
                          value
                            .split(",")
                            .map((token) => token.trim().replace(/^\./, "").toLowerCase())
                            .filter(Boolean),
                        ),
                      ],
                    },
                  }));
                }}
              />
            </div>
            <fieldset>
              <legend className="mb-2 text-sm font-medium">
                <Trans context="emulation section">Platforms</Trans>
              </legend>
              <div className="grid gap-2 sm:grid-cols-2">
                {snapshot.platforms.map((platform) => (
                  <Checkbox
                    key={platform.id}
                    isSelected={profile.platforms.includes(platform.id)}
                    onChange={(selected) =>
                      setDraft((old) => ({
                        ...old,
                        profile: {
                          ...profile,
                          platforms: selected
                            ? [...profile.platforms, platform.id]
                            : profile.platforms.filter((id) => id !== platform.id),
                        },
                      }))
                    }
                  >
                    {platform.name}
                  </Checkbox>
                ))}
              </div>
            </fieldset>
            <Select
              label={t`Playtime tracking`}
              value={profile.tracking.type}
              options={trackingModes.map((mode) => ({ id: mode.id, label: t(mode.label) }))}
              onChange={(type) =>
                setDraft((old) => ({
                  ...old,
                  profile: {
                    ...profile,
                    tracking:
                      type === "folder"
                        ? { type, path: "" }
                        : type === "process_name"
                          ? { type, name: "" }
                          : { type },
                  },
                }))
              }
            />
            {profile.tracking.type === "folder" && (
              <TextField
                label={t`Tracking folder`}
                mono
                value={profile.tracking.path}
                onChange={(path) =>
                  setDraft((old) => ({
                    ...old,
                    profile: { ...profile, tracking: { type: "folder", path } },
                  }))
                }
              />
            )}
            {profile.tracking.type === "process_name" && (
              <TextField
                label={t({ message: "Process name", context: "playtime tracking" })}
                mono
                value={profile.tracking.name}
                onChange={(name) =>
                  setDraft((old) => ({
                    ...old,
                    profile: { ...profile, tracking: { type: "process_name", name } },
                  }))
                }
              />
            )}
          </>
        ) : (
          <>
            <p className="text-sm text-fg-muted">
              {t({
                message: `Platforms: ${{
                  platforms:
                    definition?.platforms
                      .map((id) => snapshot.platforms.find((row) => row.id === id)?.name ?? id)
                      .join(", ") || unknownProfile,
                }}`,
                comment: "An emulator profile's platforms",
              })}
            </p>
            <p className="font-mono text-xs text-fg-subtle">
              {t({
                message: `File types: ${{
                  extensions: definition?.extensions.join(", ") || noExtensions,
                }}`,
                comment: "The file extensions an emulator profile opens",
              })}
            </p>
            {definition && definition.requires.length > 0 && (
              <ul className="space-y-1 text-xs text-fg-muted">
                {definition.requires.map((path) => (
                  <li key={path}>
                    <Trans comment="A file an emulator profile needs">Requires: {path}</Trans>
                  </li>
                ))}
              </ul>
            )}
            <SegmentedControl
              aria-label={t`Arguments mode`}
              options={argumentModes.map((mode) => ({ id: mode.id, label: t(mode.label) }))}
              value={profile.args.type}
              onChange={(type) =>
                setDraft((old) => ({
                  ...old,
                  // Replace starts from the specification's arguments, so it is an edit.
                  argsText:
                    type === "replace" && !old.argsText.trim()
                      ? argvText(definition?.args ?? [])
                      : old.argsText,
                  profile: { ...profile, args: type === "default" ? { type } : { type, args: [] } },
                }))
              }
            />
            {drift && profile.args.type === "replace" && (
              <div
                role="status"
                data-part="drift"
                className="space-y-2 rounded-control border border-warning/40 p-3 text-sm text-warning"
              >
                <p>
                  <Trans>
                    Caskade's arguments for this emulator changed since you replaced them.
                  </Trans>
                </p>
                <p className="break-all font-mono text-xs text-fg-muted">
                  {t({
                    message: `Replaced then: ${{
                      args: argvText(drift.overriddenFrom) || noArgs,
                    }}`,
                    comment: "The arguments a Replace override was saved with",
                  })}
                </p>
                <p className="break-all font-mono text-xs text-fg-muted">
                  {t({
                    message: `Caskade's now: ${{
                      args: argvText(drift.definitionArgs) || noArgs,
                    }}`,
                    comment: "The arguments Caskade's definition of the emulator gives now",
                  })}
                </p>
                <div className="flex flex-wrap gap-2">
                  <Button
                    size="sm"
                    onPress={() =>
                      setDraft((old) => ({
                        ...old,
                        argsText: argvText(drift.definitionArgs),
                        profile: {
                          ...profile,
                          args: { type: "replace", args: [], replaced: drift.definitionArgs },
                        },
                      }))
                    }
                  >
                    <Trans>Start from the new arguments</Trans>
                  </Button>
                  <Button
                    size="sm"
                    onPress={() =>
                      setDraft((old) => ({
                        ...old,
                        profile: {
                          ...profile,
                          args: { ...profile.args, replaced: drift.definitionArgs },
                        },
                      }))
                    }
                  >
                    <Trans>Keep my arguments</Trans>
                  </Button>
                </div>
              </div>
            )}
          </>
        )}
        {(profile.type === "custom" || profile.args.type !== "default") && (
          <>
            <TextField
              label={
                profile.type === "custom"
                  ? t`Arguments`
                  : profile.args.type === "replace"
                    ? t`Replacement arguments`
                    : t`Extra arguments`
              }
              mono
              value={draft.argsText}
              onChange={(argsText) => setDraft((old) => ({ ...old, argsText }))}
              description={t`Split into arguments as a shell would, but no shell runs. Quote a value that has spaces.`}
            />
            <div data-part="variables" className="flex flex-wrap gap-1.5">
              {variables.map((variable) => (
                <Button
                  key={variable.name}
                  size="sm"
                  variant="ghost"
                  aria-label={t({
                    message: `Insert ${{ variable: `{${variable.name}}` }}: ${{ description: variable.description }}`,
                    comment: "Inserts a launch variable into the arguments",
                  })}
                  onPress={() =>
                    setDraft((old) => ({
                      ...old,
                      argsText: `${old.argsText.trimEnd()}${old.argsText.trim() ? " " : ""}"{${variable.name}}"`,
                    }))
                  }
                >{`{${variable.name}}`}</Button>
              ))}
            </div>
          </>
        )}
        {profile.type === "custom" && (
          <fieldset>
            <legend className="mb-2 text-sm font-medium">
              <Trans comment="Environment variables">Environment</Trans>
            </legend>
            <div className="space-y-2">
              {profile.env.map(([name, value], index) => (
                <div
                  // biome-ignore lint/suspicious/noArrayIndexKey: draft environment rows remain positional until saved.
                  key={index}
                  className="grid items-end gap-2 sm:grid-cols-[1fr_2fr_auto]"
                >
                  <TextField
                    aria-label={t`Environment name ${{ row: index + 1 }}`}
                    value={name}
                    mono
                    onChange={(name) =>
                      setDraft((old) => ({
                        ...old,
                        profile: {
                          ...profile,
                          env: profile.env.map((row, i) => (i === index ? [name, row[1]] : row)),
                        },
                      }))
                    }
                  />
                  <TextField
                    aria-label={t`Environment value ${{ row: index + 1 }}`}
                    value={value}
                    mono
                    onChange={(value) =>
                      setDraft((old) => ({
                        ...old,
                        profile: {
                          ...profile,
                          env: profile.env.map((row, i) => (i === index ? [row[0], value] : row)),
                        },
                      }))
                    }
                  />
                  <Button
                    aria-label={t`Remove environment row ${{ row: index + 1 }}`}
                    onPress={() =>
                      setDraft((old) => ({
                        ...old,
                        profile: { ...profile, env: profile.env.filter((_, i) => i !== index) },
                      }))
                    }
                  >
                    <Trans context="environment row">Remove</Trans>
                  </Button>
                </div>
              ))}
            </div>
            <Button
              className="mt-2"
              size="sm"
              icon={Plus}
              onPress={() =>
                setDraft((old) => ({
                  ...old,
                  profile: { ...profile, env: [...profile.env, ["", ""]] },
                }))
              }
            >
              <Trans>Add environment variable</Trans>
            </Button>
          </fieldset>
        )}
        <div className="grid gap-4 md:grid-cols-2">
          <TextField
            label={t`Before launch`}
            description={t`Optional. A command to run first; if it fails, the game doesn't start.`}
            mono
            value={draft.beforeText}
            onChange={(beforeText) => setDraft((old) => ({ ...old, beforeText }))}
          />
          <TextField
            label={t`After exit`}
            mono
            value={draft.afterExitText}
            onChange={(afterExitText) => setDraft((old) => ({ ...old, afterExitText }))}
          />
        </div>
        <PreviewTokens
          preview={preview.data ?? current?.resolved}
          pending={preview.isFetching}
          error={preview.error?.message}
        />
        {save.error && (
          <p role="alert" className="text-sm text-danger">
            {save.error.message}
          </p>
        )}
        <div className="flex items-center justify-end gap-2">
          <UnsavedHint dirty={dirty} className="mr-auto" />
          <Button isDisabled={save.isPending} onPress={close}>
            <Trans>Cancel</Trans>
          </Button>
          <Button
            variant="primary"
            isDisabled={save.isPending}
            // A failure shows above the buttons.
            onPress={() => void submit().catch(() => {})}
          >
            {save.isPending ? t`Saving…` : t`Save profile`}
          </Button>
        </div>
        {/* The user's lifecycle scripts for this profile (#180): saved on
            their own, apart from the profile's fields above. */}
        {current && (
          <section aria-label={t`Profile scripts`} className="border-t border-border pt-4">
            <h3 className="mb-3 font-display text-base font-semibold">{t`Profile scripts`}</h3>
            <ScriptsPanel
              scope={{ type: "profile", profileId: profile.id }}
              profileId={profile.id}
            />
          </section>
        )}
      </div>
    </SettingsSection>
  );
}
