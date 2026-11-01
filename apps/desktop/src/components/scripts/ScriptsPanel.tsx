/** One scope's lifecycle scripts (#180), in the settings (global), an
 * emulator profile and the game editor: their order and what each runs,
 * a deliberate Test through the launch's own runner with its output, and
 * the order a launch takes across every scope. */
import { Button, SegmentedControl, Select, Switch, TextField } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { useMutation } from "@tanstack/react-query";
import { ArrowDown, ArrowUp, Copy, FlaskConical, Plus, Square, Trash2 } from "lucide-react";
import { useEffect, useId, useState } from "react";
import { ListBox, ListBoxItem } from "react-aria-components";
import {
  blankScript,
  type LifecycleScript,
  type ScriptPhase,
  type ScriptScope,
  type ScriptTest,
  type Shell,
  useEffectiveScripts,
  useScriptChanges,
  useScripts,
  useScriptsApi,
} from "../../api/scripts";
import { ListField, PathField } from "../gameActions/ActionForm";
import { TextAreaField } from "../gameEditor/fields";

const WINDOWS = typeof navigator !== "undefined" && /Win/i.test(navigator.platform);
/** The shells this OS has, first the one a new script uses. */
const SHELLS: Shell[] = WINDOWS
  ? ["powershell", "pwsh", "cmd", "bash", "sh"]
  : ["sh", "bash", "pwsh"];

function usePhaseLabels() {
  const { t } = useLingui();
  return {
    before_launch: t({ message: "Before launch", context: "script phase" }),
    after_start: t({ message: "After the game starts", context: "script phase" }),
    after_exit: t({ message: "After the game exits", context: "script phase" }),
  } satisfies Record<ScriptPhase, string>;
}

function useScopeLabel() {
  const { t } = useLingui();
  return (scope: ScriptScope) =>
    scope.type === "global"
      ? t({ message: "Global", context: "script scope" })
      : scope.type === "profile"
        ? t({ message: "Emulator profile", context: "script scope" })
        : t({ message: "This game", context: "script scope" });
}

/** Written as they are; never a translatable placeholder. */
const PLACEHOLDERS = "{InstallDir}, {GameTitle}";

const sameScope = (a: ScriptScope, b: ScriptScope) => JSON.stringify(a) === JSON.stringify(b);

interface ScriptsPanelProps {
  scope: ScriptScope;
  /** The game whose launch order shows, and whose values a test uses. */
  gameId?: string | null;
  profileId?: string | null;
  onDirtyChange?: (dirty: boolean) => void;
}

/** Nothing where the library has no scripts. */
export function ScriptsPanel(props: ScriptsPanelProps) {
  return useScriptsApi() ? <Scripts {...props} /> : null;
}

function Scripts({ scope, gameId = null, profileId = null, onDirtyChange }: ScriptsPanelProps) {
  const { t } = useLingui();
  const phases = usePhaseLabels();
  const query = useScripts(scope);
  const changes = useScriptChanges(scope);
  const scripts = query.data ?? [];
  const [selected, setSelected] = useState<string | null>(null);
  const [draft, setDraft] = useState<LifecycleScript | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const shown = scripts.find((script) => script.id === (selected ?? scripts[0]?.id)) ?? null;
  const editing = draft ?? shown;
  const saved = draft && draft.id !== shown?.id ? null : shown;
  const dirty = draft !== null && JSON.stringify(draft) !== JSON.stringify(saved);
  useEffect(() => onDirtyChange?.(dirty), [dirty, onDirtyChange]);
  const choose = (id: string | null) => {
    setSelected(id);
    setDraft(null);
    setConfirmDelete(false);
    changes.save.reset();
  };
  const startNew = () => {
    setSelected(null);
    changes.save.reset();
    setDraft({ ...blankScript(scope, SHELLS[0] as Shell), name: t`New script` });
  };
  const duplicate = (script: LifecycleScript) => {
    setSelected(null);
    changes.save.reset();
    // A copy starts off: copying never turns on a second command unseen.
    setDraft({
      ...structuredClone(script),
      id: "",
      enabled: false,
      name: t({ message: `${script.name} (copy)`, comment: "A duplicated script's name" }),
    });
  };
  const move = (script: LifecycleScript, by: -1 | 1) => {
    const ids = scripts.map((each) => each.id);
    const at = ids.indexOf(script.id);
    const to = at + by;
    if (at < 0 || to < 0 || to >= ids.length) return;
    ids.splice(to, 0, ...ids.splice(at, 1));
    changes.order.mutate(ids);
  };
  const failure =
    changes.save.error?.message ?? changes.remove.error?.message ?? changes.order.error?.message;
  if (query.isPending)
    return (
      <p role="status" className="text-sm text-fg-muted">
        <Trans>Loading scripts…</Trans>
      </p>
    );
  if (query.isError)
    return (
      <p role="alert" className="text-sm text-danger">
        {query.error.message}
      </p>
    );
  return (
    <div data-cs="scripts" className="flex flex-col gap-5">
      <div className="grid gap-5 md:grid-cols-[minmax(12rem,16rem)_1fr]">
        <div className="flex flex-col gap-3">
          {scripts.length > 0 ? (
            <ListBox
              data-part="items"
              aria-label={t`Scripts`}
              selectionMode="single"
              disallowEmptySelection
              selectedKeys={editing?.id ? [editing.id] : []}
              onSelectionChange={(keys) => {
                const [key] = [...keys];
                if (typeof key === "string" && !dirty) choose(key);
              }}
              className="flex flex-col gap-1"
            >
              {scripts.map((script) => (
                <ListBoxItem
                  key={script.id}
                  id={script.id}
                  textValue={script.name}
                  data-part="script"
                  data-enabled={script.enabled}
                  data-spatial="focusable"
                  isDisabled={dirty && script.id !== editing?.id}
                  className="flex cursor-pointer flex-col rounded-control border border-border px-3 py-2 text-sm outline-none data-disabled:opacity-50 data-focus-visible:focus-ring data-selected:border-accent-alt data-selected:bg-accent/10 data-[enabled=false]:text-fg-muted"
                >
                  <span className="truncate font-medium">{script.name}</span>
                  <span className="text-xs text-fg-muted">
                    {phases[script.phase]}
                    {!script.enabled && ` · ${t({ message: "Off", context: "a disabled script" })}`}
                  </span>
                </ListBoxItem>
              ))}
            </ListBox>
          ) : (
            <p className="text-sm text-fg-muted">
              <Trans>No scripts here yet.</Trans>
            </p>
          )}
          <Button data-spatial="focusable" icon={Plus} isDisabled={dirty} onPress={startNew}>
            {t`Add script`}
          </Button>
        </div>
        {editing && (
          <div className="flex flex-col gap-4">
            <ScriptForm script={editing} onChange={setDraft} isDisabled={changes.save.isPending} />
            {failure && (
              <p role="alert" data-part="error" className="text-sm text-danger">
                {failure}
              </p>
            )}
            <div className="flex flex-wrap items-center gap-2">
              <Button
                data-spatial="focusable"
                variant="primary"
                isDisabled={!dirty || changes.save.isPending}
                onPress={() =>
                  changes.save.mutate(editing, {
                    onSuccess: (script) => {
                      setSelected(script.id);
                      setDraft(null);
                    },
                  })
                }
              >
                {changes.save.isPending ? t`Saving…` : t`Save script`}
              </Button>
              <Button
                data-spatial="focusable"
                isDisabled={!dirty || changes.save.isPending}
                onPress={() => {
                  setDraft(null);
                  changes.save.reset();
                }}
              >
                {t`Revert`}
              </Button>
              {saved && (
                <>
                  <Button
                    data-spatial="focusable"
                    icon={Copy}
                    isDisabled={dirty}
                    onPress={() => duplicate(saved)}
                  >
                    {t`Duplicate`}
                  </Button>
                  <Button
                    data-spatial="focusable"
                    aria-label={t`Move up`}
                    icon={ArrowUp}
                    isDisabled={dirty || scripts[0]?.id === saved.id || changes.order.isPending}
                    onPress={() => move(saved, -1)}
                  />
                  <Button
                    data-spatial="focusable"
                    aria-label={t`Move down`}
                    icon={ArrowDown}
                    isDisabled={
                      dirty ||
                      scripts[scripts.length - 1]?.id === saved.id ||
                      changes.order.isPending
                    }
                    onPress={() => move(saved, 1)}
                  />
                  <Button
                    data-spatial="focusable"
                    data-part="delete"
                    variant="danger"
                    icon={Trash2}
                    isDisabled={changes.remove.isPending}
                    onPress={() => {
                      if (!confirmDelete) {
                        setConfirmDelete(true);
                        return;
                      }
                      changes.remove.mutate(saved.id, { onSuccess: () => choose(null) });
                    }}
                  >
                    {confirmDelete ? t`Delete “${saved.name}”?` : t`Delete`}
                  </Button>
                </>
              )}
            </div>
            <ScriptTester
              key={editing.id || "new"}
              scope={scope}
              script={editing}
              gameId={gameId}
            />
          </div>
        )}
      </div>
      <EffectiveOrder scope={scope} gameId={gameId} profileId={profileId} />
    </div>
  );
}

function ScriptForm({
  script,
  onChange,
  isDisabled,
}: {
  script: LifecycleScript;
  onChange: (script: LifecycleScript) => void;
  isDisabled?: boolean;
}) {
  const { t } = useLingui();
  const phases = usePhaseLabels();
  const set = (next: Partial<LifecycleScript>) => onChange({ ...script, ...next });
  const command = script.command;
  return (
    <div data-part="form" className="flex flex-col gap-4">
      <TextField
        label={t({ message: "Name", context: "script" })}
        value={script.name}
        onChange={(name) => set({ name })}
        isDisabled={isDisabled}
      />
      <Switch
        isSelected={script.enabled}
        onChange={(enabled) => set({ enabled })}
        isDisabled={isDisabled}
      >
        {t`Runs with launches`}
      </Switch>
      <Select<ScriptPhase>
        label={t`When`}
        options={(Object.keys(phases) as ScriptPhase[]).map((id) => ({ id, label: phases[id] }))}
        value={script.phase}
        onChange={(phase) => set({ phase })}
        isDisabled={isDisabled}
      />
      <p className="text-xs text-fg-muted">
        {script.phase === "before_launch"
          ? t`If it fails or runs out of time, the game doesn't start and the after-exit scripts clean up.`
          : script.phase === "after_start"
            ? t`Runs beside the game once it is found. A failure is noted; the game carries on.`
            : t`Runs once the game has ended, or a launch didn't start. A failure is noted.`}
      </p>
      <SegmentedControl
        aria-label={t`What it runs`}
        options={[
          { id: "shell", label: t({ message: "Shell script", context: "script kind" }) },
          { id: "direct", label: t({ message: "Program", context: "script kind" }) },
        ]}
        value={command.type}
        onChange={(kind) =>
          set({
            command:
              kind === "shell"
                ? { type: "shell", shell: SHELLS[0] as Shell, script: "" }
                : { type: "direct", program: "", args: [] },
          })
        }
        isDisabled={isDisabled}
      />
      {command.type === "shell" ? (
        <>
          <Select<Shell>
            label={t`Shell`}
            options={(SHELLS.includes(command.shell) ? SHELLS : [command.shell, ...SHELLS]).map(
              (id) => ({ id, label: id }),
            )}
            value={command.shell}
            onChange={(shell) => set({ command: { ...command, shell } })}
            isDisabled={isDisabled}
          />
          <TextAreaField
            label={t`Script`}
            value={command.script}
            onChange={(text) => set({ command: { ...command, script: text } })}
            rows={6}
          />
          <p className="text-xs text-fg-muted">
            {t`The script reads the launch's values from CASKADE_ variables, like "$CASKADE_INSTALL_DIR". Nothing is pasted into its text.`}
          </p>
        </>
      ) : (
        <>
          <PathField
            label={t`Program`}
            value={command.program}
            onChange={(program) => set({ command: { ...command, program } })}
            request={{ kind: "file", title: t`Choose the program` }}
            isDisabled={isDisabled}
          />
          <ListField
            label={t({ message: "Argument", context: "a program's command-line argument" })}
            values={command.args}
            onChange={(args) => set({ command: { ...command, args } })}
            addLabel={t`Add argument`}
            isDisabled={isDisabled}
          />
          <p className="text-xs text-fg-muted">
            {t`Each argument goes to the program as it is, with no shell: ${PLACEHOLDERS} and the other values below are put in as text.`}
          </p>
        </>
      )}
      <PathField
        label={t`Working folder`}
        description={t`Empty: the game's install folder.`}
        value={script.cwd ?? ""}
        onChange={(cwd) => set({ cwd: cwd || null })}
        request={{ kind: "folder", title: t`Choose the working folder` }}
        isDisabled={isDisabled}
      />
      <TextField
        label={t`Time limit (seconds)`}
        inputMode="numeric"
        value={String(script.timeoutSecs)}
        onChange={(value) => set({ timeoutSecs: Number.parseInt(value, 10) || 0 })}
        isDisabled={isDisabled}
      />
    </div>
  );
}

function ScriptTester({
  scope,
  script,
  gameId,
}: {
  scope: ScriptScope;
  script: LifecycleScript;
  gameId: string | null;
}) {
  const { t } = useLingui();
  const api = useScriptsApi();
  const id = useId();
  const [count, setCount] = useState(0);
  const testId = `${id}:${count}`;
  const test = useMutation({
    mutationFn: (): Promise<ScriptTest> => {
      if (!api) throw new Error("Scripts aren't available here");
      return api.test(scope, script, gameId, testId);
    },
    onSettled: () => setCount((value) => value + 1),
  });
  return (
    <div data-part="test" className="flex flex-col gap-3 border-t border-border pt-3">
      <div className="flex flex-wrap items-center gap-2">
        <Button
          data-spatial="focusable"
          icon={FlaskConical}
          isDisabled={test.isPending}
          onPress={() => test.mutate()}
        >
          {test.isPending ? t`Testing…` : t`Test`}
        </Button>
        {test.isPending && (
          <Button
            data-spatial="focusable"
            icon={Square}
            onPress={() => void api?.cancelTest(testId)}
          >
            {t({ message: "Cancel test", context: "stops a running script test" })}
          </Button>
        )}
        <span className="text-xs text-fg-muted">
          {t`Runs it once now, as a launch would, without a game starting.`}
        </span>
      </div>
      {test.isError && (
        <p role="alert" className="text-sm text-danger">
          {test.error.message}
        </p>
      )}
      {test.data && <TestResult result={test.data} />}
    </div>
  );
}

function TestResult({ result }: { result: ScriptTest }) {
  const { t } = useLingui();
  const run = result.run;
  const status = {
    succeeded: t({ message: "Finished", context: "script test result" }),
    failed:
      run.exitCode === null
        ? t`Stopped by a signal`
        : t({ message: `Failed with exit code ${run.exitCode}`, context: "script test result" }),
    timed_out: t`Ran out of time`,
    cancelled: t({ message: "Cancelled", context: "script test result" }),
    not_started: t`Didn't start`,
  }[run.status];
  return (
    <div data-part="result" data-status={run.status} className="flex flex-col gap-2 text-sm">
      <p role="status" className="font-medium">
        {status} · {t`${run.durationMs} ms`}
      </p>
      {run.error && <p className="text-danger">{run.error}</p>}
      {run.truncated && (
        <p className="text-xs text-fg-muted">{t`The output was long; its end is shown.`}</p>
      )}
      {(["stdout", "stderr"] as const).map(
        (stream) =>
          run[stream] && (
            <div key={stream}>
              <p className="text-xs font-semibold">{stream === "stdout" ? t`Output` : t`Errors`}</p>
              <pre
                data-part="output"
                className="max-h-48 overflow-auto rounded-control border border-border bg-surface-base/60 p-2 font-mono text-xs whitespace-pre-wrap"
              >
                {run[stream]}
              </pre>
            </div>
          ),
      )}
      <details data-part="context">
        <summary className="cursor-pointer text-xs font-semibold">{t`Values it saw`}</summary>
        <dl className="mt-2 grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 font-mono text-xs">
          {result.context.map((value) => (
            <div key={value.variable} className="contents">
              <dt className="text-fg-muted">
                {value.placeholder} · {value.variable}
              </dt>
              <dd className="break-all">{value.value || "—"}</dd>
            </div>
          ))}
        </dl>
      </details>
    </div>
  );
}

/** The order a launch runs scripts in, every scope's, with the ones from
 * other scopes and the disabled ones marked. */
function EffectiveOrder({
  scope,
  gameId,
  profileId,
}: {
  scope: ScriptScope;
  gameId: string | null;
  profileId: string | null;
}) {
  const { t } = useLingui();
  const phases = usePhaseLabels();
  const scopeLabel = useScopeLabel();
  const steps = useEffectiveScripts(gameId, profileId).data ?? [];
  return (
    <section data-part="order" aria-label={t`Order at launch`} className="flex flex-col gap-2">
      <h3 className="text-sm font-semibold">{t`Order at launch`}</h3>
      {steps.length === 0 ? (
        <p className="text-xs text-fg-muted">{t`No scripts run at launch.`}</p>
      ) : (
        (Object.keys(phases) as ScriptPhase[]).map((phase) => {
          const inPhase = steps.filter((step) => step.phase === phase);
          if (inPhase.length === 0) return null;
          return (
            <div key={phase}>
              <p className="text-xs font-semibold text-fg-muted">{phases[phase]}</p>
              <ol className="ml-4 list-decimal text-sm">
                {inPhase.map((step) => {
                  const inherited = !sameScope(step.script.scope, scope);
                  return (
                    <li
                      key={step.script.id}
                      data-part="step"
                      data-inherited={inherited || undefined}
                      data-enabled={step.script.enabled}
                      className="data-[enabled=false]:text-fg-subtle data-[enabled=false]:line-through"
                    >
                      {step.script.name}
                      <span className="ml-2 text-xs text-fg-muted no-underline">
                        {inherited
                          ? t`${scopeLabel(step.script.scope)}, inherited`
                          : scopeLabel(step.script.scope)}
                        {!step.script.enabled &&
                          ` · ${t({ message: "Off", context: "a disabled script" })}`}
                      </span>
                    </li>
                  );
                })}
              </ol>
            </div>
          );
        })
      )}
    </section>
  );
}
