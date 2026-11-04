import {
  Button,
  Menu,
  MenuItem,
  MenuPopover,
  Select,
  SettingsSection,
  StatusDot,
  TextField,
} from "@caskade/ui";
import type { MessageDescriptor } from "@lingui/core";
import { msg } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { Cpu, Layers, Pencil, Plus, Settings2, Trash2 } from "lucide-react";
import { useState } from "react";
import {
  Button as AriaButton,
  Disclosure,
  DisclosurePanel,
  Heading,
  ListBox,
  ListBoxItem,
  MenuTrigger,
} from "react-aria-components";
import {
  type ConfiguredEmulator,
  type EmulationSnapshot,
  type EmulatorEdit,
  type EmulatorInstall,
  emptyCustomProfile,
  installKinds,
  installLocation,
  type ProfileEdit,
  profileEdit,
} from "../../../api/emulation";
import { useActionMenus } from "../../../components/ActionMenu";
import { ConfirmDialog } from "../../../components/ConfirmDialog";
import { UnsavedHint, useUnsavedChanges } from "../unsavedChanges";
import { BiosSummary, PathField, PreviewTokens } from "./EmulationFields";
import { useEmulationWrite } from "./EmulationProvider";
import { ManagedEmulatorInfo } from "./EmulatorSetup";
import { ProfileEditor } from "./ProfileEditor";

const emptyInstall = (type: EmulatorInstall["type"]): EmulatorInstall =>
  type === "folder"
    ? { type, dir: "", executable: "" }
    : type === "path"
      ? { type, binary: "" }
      : type === "flatpak"
        ? { type, appId: "" }
        : type === "app_image"
          ? { type, file: "" }
          : { type, bundle: "" };
/** Install kinds whose names aren't product names (`installKinds`). */
const installKindLabels: Partial<Record<EmulatorInstall["type"], MessageDescriptor>> = {
  folder: msg({ message: "Folder", comment: "How an emulator is installed: in a folder" }),
  path: msg({
    message: "$PATH binary",
    comment: "How an emulator is installed: a program found on the PATH",
  }),
  app_bundle: msg({ message: "App bundle", comment: "How an emulator is installed: a macOS app" }),
};
function Status({ emulator }: { emulator: ConfiguredEmulator }) {
  const missing = emulator.profiles.filter((row) => !row.resolved.available).length;
  return !emulator.found ? (
    <StatusDot tone="danger">
      <Trans comment="An emulator's status">Not found</Trans>
    </StatusDot>
  ) : missing ? (
    <StatusDot tone="warning">
      <Trans comment="An emulator's status">Some profiles missing</Trans>
    </StatusDot>
  ) : (
    <StatusDot tone="ok">
      <Trans comment="An emulator's status">Ready</Trans>
    </StatusDot>
  );
}
function EmulatorForm({
  emulator,
  snapshot,
  close,
  created,
}: {
  emulator?: ConfiguredEmulator;
  snapshot: EmulationSnapshot;
  close: () => void;
  created: (id: string) => void;
}) {
  const { t } = useLingui();
  const save = useEmulationWrite();
  const [draft, setDraft] = useState<EmulatorEdit>(() =>
    emulator
      ? {
          id: emulator.id,
          name: emulator.name,
          definition: emulator.definition,
          install: structuredClone(emulator.install),
        }
      : {
          id: null,
          name: t({ message: "New emulator", comment: "The name a new emulator starts with" }),
          definition: null,
          install: emptyInstall("folder"),
        },
  );
  const [start] = useState(() => JSON.stringify(draft));
  const dirty = JSON.stringify(draft) !== start;
  const submit = async () => {
    const next = await save.mutateAsync((api) => api.saveEmulator(draft));
    if (!emulator) {
      const added = next.emulators.find(
        (row) => !snapshot.emulators.some((old) => old.id === row.id) && row.name === draft.name,
      );
      if (added) created(added.id);
    }
    return true;
  };
  // The emulator's fields wait for Save emulator (#43): leaving the section asks first.
  useUnsavedChanges(dirty, {
    label: emulator ? emulator.name : t`Add emulator`,
    save: submit,
    discard: () => setDraft(JSON.parse(start) as EmulatorEdit),
  });
  const setInstall = (install: EmulatorInstall) => setDraft((old) => ({ ...old, install }));
  const install = draft.install;
  return (
    <SettingsSection
      icon={Cpu}
      title={emulator ? emulator.name : t`Add emulator`}
      description={t`Installation status is checked by the backend after saving; configuration does not run an emulator.`}
    >
      <div className="mt-4 grid gap-4 md:grid-cols-2">
        <TextField
          label={t`Emulator name`}
          value={draft.name}
          onChange={(name) => setDraft((old) => ({ ...old, name }))}
        />
        <Select
          label={t`Emulator specification`}
          value={draft.definition ?? "none"}
          options={[
            { id: "none", label: t`None (custom)` },
            ...snapshot.definitions.map((row) => ({ id: row.id, label: row.name })),
          ]}
          onChange={(definition) =>
            setDraft((old) => ({ ...old, definition: definition === "none" ? null : definition }))
          }
        />
        <Select
          label={t`Install type`}
          value={install.type}
          options={installKinds.map((kind) => {
            const label = installKindLabels[kind.id];
            return { id: kind.id, label: label ? t(label) : kind.label };
          })}
          onChange={(type) => setInstall(emptyInstall(type))}
        />
        {install.type === "folder" && (
          <>
            <PathField
              label={t`Emulator folder`}
              value={install.dir}
              onChange={(dir) => setInstall({ ...install, dir })}
            />
            <PathField
              label={t`Emulator executable`}
              kind="file"
              value={install.executable}
              onChange={(executable) => setInstall({ ...install, executable })}
            />
          </>
        )}
        {install.type === "path" && (
          <TextField
            label={t`Binary name`}
            mono
            value={install.binary}
            onChange={(binary) => setInstall({ ...install, binary })}
          />
        )}
        {install.type === "flatpak" && (
          <TextField
            label={t`Flatpak app ID`}
            mono
            value={install.appId}
            onChange={(appId) => setInstall({ ...install, appId })}
          />
        )}
        {install.type === "app_image" && (
          <PathField
            label={t`AppImage file`}
            kind="file"
            value={install.file}
            onChange={(file) => setInstall({ ...install, file })}
          />
        )}
        {install.type === "app_bundle" && (
          <PathField
            label={t({ message: "App bundle", comment: "A macOS app bundle's path" })}
            value={install.bundle}
            onChange={(bundle) => setInstall({ ...install, bundle })}
          />
        )}
      </div>
      {emulator && emulator.definition !== draft.definition && (
        <p className="mt-3 text-sm text-warning">
          <Trans>
            Saving a different specification removes its old built-in profiles. Custom profiles are
            kept.
          </Trans>
        </p>
      )}
      {emulator && (
        <div className="mt-3">
          <Status emulator={emulator} />
        </div>
      )}
      {save.error && (
        <p role="alert" className="mt-3 text-sm text-danger">
          {save.error.message}
        </p>
      )}
      <div className="mt-4 flex items-center justify-end gap-2">
        <UnsavedHint dirty={dirty} className="mr-auto" />
        {!emulator && (
          <Button isDisabled={save.isPending} onPress={close}>
            <Trans>Cancel</Trans>
          </Button>
        )}
        <Button
          variant="primary"
          isDisabled={save.isPending}
          // A failure shows above the buttons.
          onPress={() => void submit().catch(() => {})}
        >
          {save.isPending ? t`Saving…` : t`Save emulator`}
        </Button>
      </div>
    </SettingsSection>
  );
}
export function ConfiguredEmulators({
  snapshot,
  selectedId,
  adding,
  select,
  add,
  added,
  cancelAdd,
}: {
  snapshot: EmulationSnapshot;
  selectedId: string | null;
  adding: boolean;
  select: (id: string) => void;
  add: () => void;
  added: (id: string) => void;
  cancelAdd: () => void;
}) {
  const { t } = useLingui();
  const noPlatforms = t({ message: "None selected", comment: "An emulator profile's platforms" });
  const noExtensions = t({ message: "None", context: "file extensions" });
  const selected = snapshot.emulators.find((row) => row.id === selectedId) ?? snapshot.emulators[0];
  const [edit, setEdit] = useState<{ emulatorId: string; value: ProfileEdit } | null>(null);
  // A removal asks first (#49): an emulator takes its profiles and the
  // platform defaults that chose it.
  const [removing, setRemoving] = useState<
    | { kind: "emulator"; emulatorId: string; name: string }
    | { kind: "profile"; emulatorId: string; profileId: string; name: string }
    | null
  >(null);
  const write = useEmulationWrite();
  const askRemove = (next: NonNullable<typeof removing>) => {
    write.reset();
    setRemoving(next);
  };
  const definition = snapshot.definitions.find((row) => row.id === selected?.definition);
  // A right click on an emulator or a profile offers its buttons.
  const menus = useActionMenus();
  return (
    <div
      data-cs="configured-emulators"
      className="grid items-start gap-4 lg:grid-cols-[19rem_minmax(0,1fr)]"
    >
      <SettingsSection
        icon={Cpu}
        title={t({ message: "Emulators", context: "emulation section" })}
        description={t({
          message: `${{ count: snapshot.emulators.length }} configured`,
          comment: "How many emulators are configured",
        })}
      >
        <ListBox
          aria-label={t({ message: "Emulators", context: "emulation section" })}
          selectionMode="single"
          selectedKeys={!adding && selected ? [selected.id] : []}
          onSelectionChange={(keys) => {
            const id = [...keys][0];
            if (id) {
              setEdit(null);
              select(String(id));
            }
          }}
          className="mt-3 space-y-2 outline-none"
        >
          {snapshot.emulators.map((emulator) => (
            <ListBoxItem
              key={emulator.id}
              id={emulator.id}
              textValue={emulator.name}
              {...menus.target(emulator.name, [
                {
                  id: "open",
                  label: t`Show settings`,
                  icon: Settings2,
                  onAction: () => {
                    setEdit(null);
                    select(emulator.id);
                  },
                },
                {
                  id: "remove",
                  label: t`Remove emulator`,
                  icon: Trash2,
                  tone: "danger",
                  isDisabled: write.isPending,
                  onAction: () =>
                    askRemove({ kind: "emulator", emulatorId: emulator.id, name: emulator.name }),
                },
              ])}
              data-cs="emulator-item"
              className="cursor-default rounded-control border border-border p-3 outline-none data-selected:border-accent-alt data-selected:bg-accent/10 data-focus-visible:focus-ring"
            >
              <p className="font-medium">{emulator.name}</p>
              <p className="mt-1 break-all font-mono text-xs text-fg-subtle">
                {installLocation(emulator.install)}
              </p>
              <div className="mt-2">
                <Status emulator={emulator} />
              </div>
            </ListBoxItem>
          ))}
        </ListBox>
        <Button className="mt-3 w-full" icon={Plus} onPress={add}>
          <Trans>Add emulator</Trans>
        </Button>
      </SettingsSection>
      {adding ? (
        <EmulatorForm key="new" snapshot={snapshot} close={cancelAdd} created={added} />
      ) : selected ? (
        <div className="min-w-0 space-y-4">
          <ManagedEmulatorInfo emulatorId={selected.id} />
          <EmulatorForm
            key={selected.id}
            emulator={selected}
            snapshot={snapshot}
            close={() => {}}
            created={() => {}}
          />
          <SettingsSection
            icon={Layers}
            title={t({ message: "Profiles", context: "emulator profiles" })}
            description={t`Availability, required files and command lines come from the backend for this installation.`}
          >
            {!selected.profiles.length && (
              <p className="mt-3 rounded-control border border-dashed border-border p-5 text-sm text-fg-subtle">
                <Trans>
                  No profiles yet. Add a built-in or custom profile before launching games.
                </Trans>
              </p>
            )}
            <div className="mt-3 space-y-3">
              {selected.profiles.map(({ config, resolved }) => (
                <Disclosure
                  key={config.id}
                  {...menus.target(resolved.name, [
                    {
                      id: "edit",
                      label: t`Edit ${{ profile: resolved.name }}`,
                      icon: Pencil,
                      onAction: () =>
                        setEdit({ emulatorId: selected.id, value: profileEdit(config) }),
                    },
                    {
                      id: "remove",
                      label: t`Remove ${{ profile: resolved.name }}`,
                      icon: Trash2,
                      tone: "danger",
                      isDisabled: write.isPending,
                      onAction: () =>
                        askRemove({
                          kind: "profile",
                          emulatorId: selected.id,
                          profileId: config.id,
                          name: resolved.name,
                        }),
                    },
                  ])}
                  data-cs="emulator-profile"
                  className="rounded-control border border-border bg-surface-overlay/40"
                >
                  <Heading>
                    <AriaButton
                      slot="trigger"
                      className="flex w-full flex-wrap items-center justify-between gap-2 p-3 text-left outline-none data-focus-visible:focus-ring"
                    >
                      <span className="font-medium">{resolved.name}</span>
                      <StatusDot
                        tone={resolved.available ? "ok" : selected.found ? "warning" : "danger"}
                      >
                        {resolved.available
                          ? t({ message: "Ready", comment: "An emulator's status" })
                          : selected.found
                            ? t({
                                message: "Missing files",
                                comment: "An emulator profile's status",
                              })
                            : t`Emulator not found`}
                      </StatusDot>
                    </AriaButton>
                  </Heading>
                  <DisclosurePanel>
                    <div className="space-y-3 border-t border-border p-3">
                      <p className="text-xs text-fg-muted">
                        {t({
                          message: `Platforms: ${{
                            platforms:
                              resolved.platforms
                                .map(
                                  (id) =>
                                    snapshot.platforms.find((row) => row.id === id)?.short ?? id,
                                )
                                .join(", ") || noPlatforms,
                          }}`,
                          comment: "An emulator profile's platforms",
                        })}
                      </p>
                      <p className="font-mono text-xs text-fg-subtle">
                        {t({
                          message: `File types: ${{
                            extensions: resolved.extensions.join(", ") || noExtensions,
                          }}`,
                          comment: "The file extensions an emulator profile opens",
                        })}
                      </p>
                      {resolved.missing.length > 0 && (
                        <ul className="space-y-1 font-mono text-xs text-warning">
                          {resolved.missing.map((path) => (
                            <li key={path}>
                              <Trans comment="A file an emulator profile needs">
                                Missing: {path}
                              </Trans>
                            </li>
                          ))}
                        </ul>
                      )}
                      {resolved.drift && (
                        <p data-part="drift" className="text-sm text-warning">
                          <Trans>
                            The specification's arguments changed since this Replace override was
                            saved. Edit the profile to review them.
                          </Trans>
                        </p>
                      )}
                      <BiosSummary files={resolved.bios} />
                      <PreviewTokens preview={resolved} />
                      <div className="flex flex-wrap justify-end gap-2">
                        <Button
                          size="sm"
                          onPress={() =>
                            setEdit({ emulatorId: selected.id, value: profileEdit(config) })
                          }
                        >
                          {t`Edit ${{ profile: resolved.name }}`}
                        </Button>
                        <Button
                          size="sm"
                          variant="danger"
                          isDisabled={write.isPending}
                          onPress={() =>
                            askRemove({
                              kind: "profile",
                              emulatorId: selected.id,
                              profileId: config.id,
                              name: resolved.name,
                            })
                          }
                        >
                          {t`Remove ${{ profile: resolved.name }}`}
                        </Button>
                      </div>
                    </div>
                  </DisclosurePanel>
                </Disclosure>
              ))}
            </div>
            <div className="mt-3 flex flex-wrap gap-2">
              {definition && (
                <MenuTrigger>
                  <Button icon={Plus} isDisabled={!selected.definitionProfiles.length}>
                    <Trans>Add built-in profile</Trans>
                  </Button>
                  <MenuPopover>
                    <Menu
                      aria-label={t`Available definition profiles`}
                      onAction={(name) =>
                        setEdit({
                          emulatorId: selected.id,
                          value: {
                            profile: {
                              type: "built_in",
                              id: "",
                              definitionProfile: String(name),
                              args: { type: "default" },
                              hooks: { before: null, afterExit: null },
                            },
                            argsText: "",
                            beforeText: "",
                            afterExitText: "",
                          },
                        })
                      }
                    >
                      {[...selected.definitionProfiles]
                        .sort((a, b) => Number(b.available) - Number(a.available))
                        .map((profile) => (
                          <MenuItem
                            id={profile.name}
                            key={profile.name}
                            description={
                              profile.available
                                ? t({
                                    message: "Available",
                                    comment: "An emulator profile's status",
                                  })
                                : t({
                                    message: "Missing files",
                                    comment: "An emulator profile's status",
                                  })
                            }
                          >
                            {profile.name}
                          </MenuItem>
                        ))}
                    </Menu>
                  </MenuPopover>
                </MenuTrigger>
              )}
              <Button
                icon={Plus}
                variant="ghost"
                onPress={() => setEdit({ emulatorId: selected.id, value: emptyCustomProfile() })}
              >
                <Trans>Add custom profile</Trans>
              </Button>
            </div>
            {write.error && (
              <p role="alert" className="mt-3 text-sm text-danger">
                {write.error.message}
              </p>
            )}
          </SettingsSection>
          {edit?.emulatorId === selected.id && (
            <ProfileEditor
              key={
                edit.value.profile.id ||
                `${edit.value.profile.type}:${edit.value.profile.type === "built_in" ? edit.value.profile.definitionProfile : "new"}`
              }
              emulator={selected}
              initial={edit.value}
              snapshot={snapshot}
              close={() => setEdit(null)}
            />
          )}
          <Button
            variant="danger"
            icon={Trash2}
            isDisabled={write.isPending}
            onPress={() =>
              askRemove({ kind: "emulator", emulatorId: selected.id, name: selected.name })
            }
          >
            <Trans>Remove emulator</Trans>
          </Button>
        </div>
      ) : (
        <div className="rounded-panel border border-dashed border-border p-8 text-sm text-fg-muted">
          <Trans>No emulators yet. Detect installed emulators or add one by hand.</Trans>
        </div>
      )}
      {menus.menu}
      {removing && (
        <ConfirmDialog
          title={t`Remove ${{ name: removing.name }}?`}
          confirmLabel={t({
            message: "Remove",
            comment: "Confirms removing an emulator or profile",
          })}
          pendingLabel={t`Removing…`}
          isPending={write.isPending}
          error={write.error}
          onConfirm={() =>
            write.mutate(
              (api) =>
                removing.kind === "emulator"
                  ? api.removeEmulator(removing.emulatorId)
                  : api.removeProfile(removing.emulatorId, removing.profileId),
              {
                onSuccess: () => {
                  if (removing.kind === "emulator") setEdit(null);
                  setRemoving(null);
                },
              },
            )
          }
          close={() => setRemoving(null)}
        >
          {removing.kind === "emulator" ? (
            <p>
              <Trans>
                Its profiles go with it, and platforms that played with it need another emulator.
                Its own files stay where they are.
              </Trans>
            </p>
          ) : (
            <p>
              <Trans>Games that play with this profile need another one.</Trans>
            </p>
          )}
        </ConfirmDialog>
      )}
    </div>
  );
}
