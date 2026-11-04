/** Appearance → Theme (#140): every installed theme with its name, author
 * and previews; one that can't be used shows why and can't be picked. The
 * choice applies at once (`AppearanceRoot` swaps the stylesheet). */
import { Button } from "@caskade/ui";
import type { MessageDescriptor } from "@lingui/core";
import { msg } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Check, FolderOpen, RefreshCw } from "lucide-react";
import { useContext, useEffect } from "react";
import { Radio, RadioGroup } from "react-aria-components";
import { effectSubscription } from "../../../api/subscription";
import { useActionMenus } from "../../../components/ActionMenu";
import { usePathPicker } from "../../../components/pathBrowser/PathPicker";
import { useDropZone } from "../../../drop/DropProvider";
import { type InstalledTheme, ThemesApiContext } from "../../../themes/api";
import { useActiveTheme } from "../../../themes/apply";

const themesKey = ["themes"] as const;

const sources: Record<string, MessageDescriptor> = {
  user: msg({ message: "Installed", context: "theme source" }),
  plugin: msg({ message: "From a plugin", context: "theme source" }),
  dev: msg({ message: "Dev folder", context: "theme source" }),
};

function problemText(problems: InstalledTheme["problems"]) {
  return problems
    .map((problem) => {
      const at = [problem.file, problem.line, problem.column].filter((part) => part != null);
      return `${at.join(":")}: ${problem.message}`;
    })
    .join("\n");
}

function ThemeOption({
  theme,
  value,
  menu,
}: {
  theme: InstalledTheme;
  value: string;
  /** Opens its right-click menu. */
  menu: object;
}) {
  const { t } = useLingui();
  const broken = theme.problems.length > 0;
  const source = sources[theme.source];
  return (
    <Radio
      {...menu}
      value={value}
      isDisabled={broken}
      className="flex cursor-pointer flex-col gap-2 rounded-card border border-border bg-surface-overlay/60 p-3 transition-colors duration-(--cs-motion-duration-fast) data-disabled:cursor-not-allowed data-focus-visible:focus-ring data-hovered:border-border-strong data-selected:border-accent data-selected:bg-surface-hover"
    >
      {theme.previews.length > 0 ? (
        <div className="grid auto-cols-fr grid-flow-col gap-1">
          {theme.previews.map((preview) => (
            <img
              key={preview}
              src={preview}
              alt=""
              className="aspect-video w-full rounded-control border border-border bg-surface-base object-cover"
            />
          ))}
        </div>
      ) : (
        <div
          aria-hidden="true"
          className="aspect-video w-full rounded-control border border-border bg-surface-base"
        />
      )}
      <span className="flex items-baseline justify-between gap-2">
        <span className="truncate font-display font-semibold tracking-wide">{theme.name}</span>
        {theme.version && (
          <span className="shrink-0 font-mono text-xs text-fg-subtle">{theme.version}</span>
        )}
      </span>
      {theme.author && (
        <span className="text-xs text-fg-muted">
          <Trans comment="A theme's author">by {theme.author}</Trans>
        </span>
      )}
      {theme.description && <span className="text-xs text-fg-muted">{theme.description}</span>}
      {source && (
        <span className="font-mono text-[0.7rem] tracking-wider text-accent-alt uppercase">
          {t(source)}
        </span>
      )}
      {broken && (
        <span className="text-xs whitespace-pre-line text-danger">
          {t`Can't be used:`}
          {"\n"}
          {problemText(theme.problems)}
        </span>
      )}
      {!broken && theme.warnings && theme.warnings.length > 0 && (
        <span className="text-xs whitespace-pre-line text-warning">
          {problemText(theme.warnings)}
        </span>
      )}
    </Radio>
  );
}

export function ThemePicker({
  value,
  onChange,
}: {
  value: string;
  onChange: (theme: string) => void;
}) {
  const { t } = useLingui();
  const api = useContext(ThemesApiContext);
  const client = useQueryClient();
  const themes = useQuery({ queryKey: themesKey, queryFn: () => api.list() });
  const active = useActiveTheme();
  // A dev theme's edits can break or fix it: list its problems afresh.
  useEffect(
    () =>
      effectSubscription(api.onReloaded, () => {
        void client.invalidateQueries({ queryKey: themesKey });
      }),
    [api, client],
  );
  const rescan = useMutation({
    mutationFn: () => api.reload(),
    onSuccess: (list) => client.setQueryData(themesKey, list),
  });
  const loadUnpacked = api.loadUnpacked;
  const picker = usePathPicker();
  const load = useMutation({
    mutationFn: async (dropped?: string) => {
      if (!loadUnpacked) return null;
      // Console mode picks the folder in the app's browser; desktop mode
      // leaves it to the native dialog `loadUnpacked` opens.
      const folder =
        dropped ??
        (picker.inApp
          ? ((await picker.pick({ kind: "folder", title: t`Theme folder` })) ?? undefined)
          : undefined);
      if (picker.inApp && !folder) return null;
      return loadUnpacked(folder);
    },
    onSuccess: async (id) => {
      await client.invalidateQueries({ queryKey: themesKey });
      if (id) onChange(id);
    },
  });
  const notice =
    active?.notice && (active.safeMode || active.requested === value) ? active.notice : null;
  const error = themes.error ?? rescan.error ?? load.error;
  // A right click on a theme offers to use it.
  const menus = useActionMenus();
  // Development builds: a theme folder dropped on the picker loads too.
  const drop = useDropZone<HTMLDivElement>(
    loadUnpacked
      ? (paths) => {
          if (paths[0]) load.mutate(paths[0]);
        }
      : null,
  );
  return (
    <div ref={drop.ref} className="flex flex-col gap-3 py-2">
      {notice && (
        <p
          role="status"
          className="rounded-control border border-warning/40 bg-warning/10 px-3 py-2 text-sm"
        >
          {notice}
        </p>
      )}
      <RadioGroup
        aria-label={t`Theme`}
        value={value}
        onChange={onChange}
        className="grid gap-3 sm:grid-cols-2 2xl:grid-cols-3"
      >
        {(themes.data ?? []).map((theme, index) => (
          <ThemeOption
            key={`${theme.source}:${theme.folder ?? theme.id}`}
            theme={theme}
            // Broken themes (a duplicate id among them) never match the choice.
            value={theme.problems.length > 0 ? `unusable:${index}` : theme.id}
            menu={menus.target(theme.name, [
              {
                id: "use",
                label: t`Use this theme`,
                icon: Check,
                isDisabled: theme.problems.length > 0 || theme.id === value,
                description: theme.problems.length > 0 ? t`It has problems` : undefined,
                onAction: () => onChange(theme.id),
              },
            ])}
          />
        ))}
      </RadioGroup>
      {menus.menu}
      <div className="flex flex-wrap items-center justify-end gap-2">
        {error && <span className="mr-auto text-xs text-danger">{String(error)}</span>}
        <Button
          variant="ghost"
          size="sm"
          icon={RefreshCw}
          onPress={() => rescan.mutate()}
          isDisabled={rescan.isPending}
        >
          <Trans>Rescan themes</Trans>
        </Button>
        {loadUnpacked && (
          <Button
            variant="ghost"
            size="sm"
            icon={FolderOpen}
            onPress={() => load.mutate(undefined)}
            isDisabled={load.isPending}
          >
            <Trans>Load theme folder</Trans>
          </Button>
        )}
      </div>
    </div>
  );
}
