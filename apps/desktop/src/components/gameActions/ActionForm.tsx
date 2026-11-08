/** One launch action's fields (#178): its name, whether it plays the game
 * or is a tool, what it starts, its arguments, working folder and
 * environment, the Proton build a Windows program runs with, and how a
 * play action's game is tracked. Nothing is typed as a command line: each
 * argument is its own field. */
import { Button, SegmentedControl, Select, TextField } from "@caskade/ui";
import { useLingui } from "@lingui/react/macro";
import { FolderOpen, Plus, X } from "lucide-react";
import type { ActionTarget, GameAction, ProtonBuilds, TrackingMode } from "../../api/actions";
import { useDropZone } from "../../drop/DropProvider";
import { type PathRequest, usePathPicker } from "../pathBrowser/PathPicker";

type TargetKind = "executable" | "uri" | "flatpak";

/** The Proton select's value for the automatic choice. */
const AUTOMATIC = "";

/** Which Proton build a Windows program runs with: the best one installed
 * when it starts, or one the user keeps it on. */
function ProtonField({
  value,
  builds,
  onChange,
  isDisabled,
}: {
  value: string | null;
  builds: ProtonBuilds;
  onChange: (value: string | null) => void;
  isDisabled?: boolean;
}) {
  const { t } = useLingui();
  const automatic = builds.automatic;
  const options = [
    {
      id: AUTOMATIC,
      label: automatic
        ? t({
            message: `Automatic (${automatic})`,
            comment: "The Proton build choice that runs the best one installed, which is named",
          })
        : t({ message: "Automatic", context: "Proton build" }),
    },
    ...builds.installed.map((name) => ({ id: name, label: name })),
  ];
  // A build another device chose, or one since removed, stays shown.
  if (value && !builds.installed.includes(value))
    options.push({
      id: value,
      label: t({
        message: `${value} (not installed here)`,
        comment: "A chosen Proton build this device doesn't have",
      }),
    });
  return (
    <div data-part="proton" className="flex flex-col gap-1.5">
      <Select<string>
        label={t`Proton build`}
        options={options}
        value={value ?? AUTOMATIC}
        onChange={(next) => onChange(next === AUTOMATIC ? null : next)}
        isDisabled={isDisabled}
      />
      <p className="text-xs text-fg-muted">
        {value && !builds.installed.includes(value)
          ? t`This device doesn't have it, so the best build installed runs instead.`
          : t`Automatic runs the best Proton build installed each time the game starts. Choose a build to keep this action on it.`}
      </p>
    </div>
  );
}

/** A path field with Browse…, which a file or folder dropped on it fills. */
export function PathField({
  label,
  description,
  value,
  onChange,
  request,
  isDisabled,
}: {
  label: string;
  description?: string;
  value: string;
  onChange: (value: string) => void;
  request: PathRequest;
  isDisabled?: boolean;
}) {
  const { t } = useLingui();
  const picker = usePathPicker();
  const drop = useDropZone<HTMLDivElement>(
    isDisabled
      ? null
      : (paths) => {
          if (paths[0]) onChange(paths[0]);
        },
  );
  return (
    <div ref={drop.ref} data-part="path" data-drop-active={drop.active || undefined}>
      <TextField
        label={label}
        description={description}
        value={value}
        onChange={onChange}
        isDisabled={isDisabled}
        mono
        action={
          <Button
            data-spatial="focusable"
            icon={FolderOpen}
            isDisabled={isDisabled}
            onPress={() =>
              void picker.pick({ ...request, startAt: value || request.startAt }).then((path) => {
                if (path) onChange(path);
              })
            }
          >
            {t`Browse…`}
          </Button>
        }
      />
    </div>
  );
}

/** A list of values, one field each: arguments, process names. */
export function ListField({
  label,
  values,
  onChange,
  addLabel,
  isDisabled,
}: {
  label: string;
  values: string[];
  onChange: (values: string[]) => void;
  addLabel: string;
  isDisabled?: boolean;
}) {
  const { t } = useLingui();
  return (
    <fieldset data-part="list" className="flex flex-col gap-2">
      <legend className="mb-1.5 text-sm font-medium">{label}</legend>
      {values.map((value, index) => (
        // Positions are the identity: the list has no other.
        // biome-ignore lint/suspicious/noArrayIndexKey: see above
        <div key={index} className="flex items-end gap-2">
          <TextField
            aria-label={t`${label} ${index + 1}`}
            value={value}
            onChange={(next) => onChange(values.map((old, at) => (at === index ? next : old)))}
            isDisabled={isDisabled}
            mono
            className="flex-1"
          />
          <Button
            data-spatial="focusable"
            data-part="remove"
            aria-label={t`Remove ${label} ${index + 1}`}
            icon={X}
            isDisabled={isDisabled}
            onPress={() => onChange(values.filter((_, at) => at !== index))}
          />
        </div>
      ))}
      <Button
        data-spatial="focusable"
        icon={Plus}
        isDisabled={isDisabled}
        onPress={() => onChange([...values, ""])}
        className="self-start"
      >
        {addLabel}
      </Button>
    </fieldset>
  );
}

/** Environment variables as name and value rows. */
function EnvField({
  env,
  onChange,
  isDisabled,
}: {
  env: Record<string, string>;
  onChange: (env: Record<string, string>) => void;
  isDisabled?: boolean;
}) {
  const { t } = useLingui();
  const rows = Object.entries(env);
  const set = (next: [string, string][]) => onChange(Object.fromEntries(next));
  return (
    <fieldset data-part="env" className="flex flex-col gap-2">
      <legend className="mb-1.5 text-sm font-medium">{t`Environment variables`}</legend>
      {rows.map(([name, value], index) => (
        // biome-ignore lint/suspicious/noArrayIndexKey: a renamed row keeps its place
        <div key={index} className="flex items-end gap-2">
          <TextField
            aria-label={t`Variable ${index + 1} name`}
            value={name}
            onChange={(next) => set(rows.map((row, at) => (at === index ? [next, row[1]] : row)))}
            isDisabled={isDisabled}
            mono
            className="flex-1"
          />
          <TextField
            aria-label={t`Variable ${index + 1} value`}
            value={value}
            onChange={(next) => set(rows.map((row, at) => (at === index ? [row[0], next] : row)))}
            isDisabled={isDisabled}
            mono
            className="flex-1"
          />
          <Button
            data-spatial="focusable"
            data-part="remove"
            aria-label={t`Remove variable ${index + 1}`}
            icon={X}
            isDisabled={isDisabled}
            onPress={() => set(rows.filter((_, at) => at !== index))}
          />
        </div>
      ))}
      <Button
        data-spatial="focusable"
        icon={Plus}
        // An empty name is a new row being filled in.
        isDisabled={isDisabled || "" in env}
        onPress={() => onChange({ ...env, "": "" })}
        className="self-start"
      >
        {t`Add variable`}
      </Button>
    </fieldset>
  );
}

function retarget(target: ActionTarget, kind: TargetKind): ActionTarget {
  if (target.type === kind) return target;
  const args = "args" in target ? (target as { args: string[] }).args : [];
  switch (kind) {
    case "executable":
      return { type: "executable", path: "", args, cwd: null, env: {} };
    case "uri":
      return { type: "uri", uri: "" };
    case "flatpak":
      return { type: "flatpak", appId: "", args };
  }
}

export function ActionForm({
  action,
  onChange,
  /** The target is the release's source's: only name and tracking change. */
  targetLocked,
  /** The Proton builds to choose from, when the action runs a Windows
   * program through Proton. */
  proton,
  isDisabled,
}: {
  action: GameAction;
  onChange: (action: GameAction) => void;
  targetLocked: boolean;
  proton?: ProtonBuilds | null;
  isDisabled?: boolean;
}) {
  const { t } = useLingui();
  const target = action.target;
  const set = (next: Partial<GameAction>) => onChange({ ...action, ...next });
  const setTarget = (next: ActionTarget) => set({ target: next });
  const locked = isDisabled || targetLocked;
  const editableKind =
    target.type === "executable" || target.type === "uri" || target.type === "flatpak";
  const trackingModes: { id: TrackingMode; label: string }[] = [
    { id: "automatic", label: t({ message: "Automatic", context: "game tracking" }) },
    { id: "process_tree", label: t`Everything it starts` },
    { id: "original_process", label: t`Only the program it starts` },
    { id: "folder", label: t`Programs in a folder` },
    { id: "process_name", label: t`Programs by name` },
  ];
  return (
    <div data-part="form" className="flex flex-col gap-4">
      <TextField
        label={t({ message: "Name", context: "launch action" })}
        value={action.name}
        onChange={(name) => set({ name })}
        isDisabled={isDisabled}
      />
      <SegmentedControl
        aria-label={t`What it does`}
        options={[
          { id: "play", label: t({ message: "Plays the game", context: "launch action kind" }) },
          { id: "utility", label: t({ message: "Tool", context: "launch action kind" }) },
        ]}
        value={action.kind}
        onChange={(kind) => set({ kind })}
        isDisabled={isDisabled || targetLocked}
      />
      {action.kind === "utility" && (
        <p className="text-xs text-fg-muted">
          {t`A tool runs from the game's menu. It counts no playtime and doesn't change when the game was last played.`}
        </p>
      )}
      {targetLocked && (
        <p data-part="locked" className="text-xs text-fg-muted">
          {t`This launch comes from where the game was found, which keeps it up to date. Duplicate it to change what it starts.`}
        </p>
      )}
      {editableKind ? (
        <Select<TargetKind>
          label={t`Starts`}
          options={[
            { id: "executable", label: t`A program` },
            { id: "uri", label: t`A launcher link` },
            { id: "flatpak", label: t`A Flatpak app` },
          ]}
          value={target.type as TargetKind}
          onChange={(kind) => setTarget(retarget(target, kind))}
          isDisabled={locked}
        />
      ) : (
        <p data-part="target-kind" className="text-sm text-fg-muted">
          {target.type === "emulator"
            ? t`Starts the game in its emulator.`
            : target.type === "web"
              ? t`Opens the web game in its game window.`
              : t`Starts the game through a plugin.`}
        </p>
      )}
      {target.type === "executable" && (
        <>
          <PathField
            label={t`Program`}
            description={t`A program or script file. Inside a Caskade install, a path relative to its folder moves with it.`}
            value={target.path}
            onChange={(path) => setTarget({ ...target, path })}
            request={{ kind: "file", title: t`Choose the program` }}
            isDisabled={locked}
          />
          <ListField
            label={t({ message: "Argument", context: "a program's command-line argument" })}
            values={target.args}
            onChange={(args) => setTarget({ ...target, args })}
            addLabel={t`Add argument`}
            isDisabled={locked}
          />
          <PathField
            label={t`Working folder`}
            description={t`Empty: the game's install folder.`}
            value={target.cwd ?? ""}
            onChange={(cwd) => setTarget({ ...target, cwd: cwd || null })}
            request={{ kind: "folder", title: t`Choose the working folder` }}
            isDisabled={locked}
          />
          <EnvField
            env={target.env}
            onChange={(env) => setTarget({ ...target, env })}
            isDisabled={locked}
          />
        </>
      )}
      {target.type === "uri" && (
        <TextField
          label={t`Link`}
          description={t`A game launcher's link, like steam://rungameid/620.`}
          value={target.uri}
          onChange={(uri) => setTarget({ ...target, uri })}
          isDisabled={locked}
          mono
        />
      )}
      {target.type === "flatpak" && (
        <>
          <TextField
            label={t`Flatpak app id`}
            description={t`Like org.example.Game.`}
            value={target.appId}
            onChange={(appId) => setTarget({ ...target, appId })}
            isDisabled={locked}
            mono
          />
          <ListField
            label={t({ message: "Argument", context: "a program's command-line argument" })}
            values={target.args}
            onChange={(args) => setTarget({ ...target, args })}
            addLabel={t`Add argument`}
            isDisabled={locked}
          />
        </>
      )}
      {proton && (
        <ProtonField
          value={action.proton}
          builds={proton}
          onChange={(build) => set({ proton: build })}
          isDisabled={isDisabled}
        />
      )}
      {action.kind === "play" && (
        <fieldset data-part="tracking" className="flex flex-col gap-3">
          <Select<TrackingMode>
            label={t`Which programs are the game`}
            options={trackingModes}
            value={action.tracking.mode}
            onChange={(mode) => set({ tracking: { ...action.tracking, mode } })}
            isDisabled={isDisabled}
          />
          <p className="text-xs text-fg-muted">
            {action.tracking.mode === "automatic"
              ? t`Caskade finds the game the usual way: its install folder and what the store says.`
              : action.tracking.mode === "process_tree"
                ? t`Playtime counts while the program or anything it started runs.`
                : action.tracking.mode === "original_process"
                  ? t`Playtime counts only while the program Caskade started runs.`
                  : action.tracking.mode === "folder"
                    ? t`Playtime counts while a program from this folder runs: for a mod manager, the game's folder.`
                    : t`Playtime counts while a program with one of these names runs: for a mod manager, the game's own program.`}
          </p>
          {action.tracking.mode === "folder" && (
            <PathField
              label={t`Folder`}
              description={t`Empty: the game's install folder.`}
              value={action.tracking.folder ?? ""}
              onChange={(folder) =>
                set({ tracking: { ...action.tracking, folder: folder || null } })
              }
              request={{ kind: "folder", title: t`Choose the game's folder` }}
              isDisabled={isDisabled}
            />
          )}
          {action.tracking.mode === "process_name" && (
            <ListField
              label={t`Program name`}
              values={action.tracking.names}
              onChange={(names) => set({ tracking: { ...action.tracking, names } })}
              addLabel={t`Add program name`}
              isDisabled={isDisabled}
            />
          )}
        </fieldset>
      )}
    </div>
  );
}
