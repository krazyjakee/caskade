import { Button, TextField } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { useMutation } from "@tanstack/react-query";
import { FolderOpen } from "lucide-react";
import type { ArgvPreview, BiosStatus } from "../../../api/emulation";
import { formatPath } from "../../../api/presentation";
import { usePathPicker } from "../../../components/pathBrowser/PathPicker";
import { useDropZone } from "../../../drop/DropProvider";
import { useEmulationApi } from "./EmulationProvider";

export function PathField({
  label,
  value,
  onChange,
  onCommit,
  kind = "folder",
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  /** Saves the path where the field saves itself: on leaving it, Enter, a
   * chosen or a dropped path. Without it a form's Save does. */
  onCommit?: (value: string) => void;
  kind?: "folder" | "file";
}) {
  const { t } = useLingui();
  const api = useEmulationApi();
  const picker = usePathPicker();
  const choose = useMutation({
    mutationFn: () =>
      picker.pick(
        { kind, title: label, startAt: kind === "folder" ? value.trim() || null : null },
        () => api.choosePath(kind),
      ),
    onSuccess: (path) => {
      if (path === null) return;
      onChange(path);
      onCommit?.(path);
    },
  });
  // A file or folder dropped on the field fills it in, like the picker.
  const drop = useDropZone<HTMLDivElement>((paths) => {
    onChange(paths[0] ?? "");
    onCommit?.(paths[0] ?? "");
  });
  return (
    <div ref={drop.ref}>
      <TextField
        label={label}
        value={formatPath(value)}
        onChange={onChange}
        onBlur={onCommit && (() => onCommit(value))}
        onKeyDown={
          onCommit &&
          ((event) => {
            if (event.key === "Enter") onCommit(value);
          })
        }
        mono
        action={
          <Button
            aria-label={t({
              message: `Browse for ${label}`,
              comment: "A path field's browse button; {label} is the field's own label",
            })}
            icon={FolderOpen}
            isDisabled={choose.isPending}
            onPress={() => choose.mutate()}
          />
        }
      />
      {choose.error && (
        <p role="alert" className="mt-1 text-xs text-danger">
          {choose.error.message}
        </p>
      )}
    </div>
  );
}
export function BiosSummary({ files }: { files: BiosStatus[] }) {
  const { t } = useLingui();
  if (!files.length)
    return (
      <span className="text-xs text-fg-subtle">
        <Trans comment="No BIOS files are needed">Not needed</Trans>
      </span>
    );
  const found = files.filter((file) => file.found === true).length;
  const unknown = files.filter((file) => file.found === null).length;
  const missingRequired = files.some((file) => file.required && file.found === false);
  return (
    <div
      data-cs="emulation-bios"
      data-status={missingRequired ? "missing" : unknown ? "unknown" : "checked"}
    >
      <p className={`text-xs ${missingRequired ? "text-danger" : "text-fg-muted"}`}>
        {unknown === files.length
          ? t({ message: "Not checked", comment: "A BIOS file's state" })
          : unknown
            ? t({
                message: `${found}/${{ total: files.length }} found · ${unknown} not checked`,
                comment: "BIOS files found out of all the platform needs",
              })
            : t({
                message: `${found}/${{ total: files.length }} found`,
                comment: "BIOS files found out of all the platform needs",
              })}
      </p>
      <ul className="mt-1 space-y-1 font-mono text-[11px] text-fg-subtle">
        {files.map((file) => (
          <li key={file.file}>
            {file.file} ·{" "}
            {file.required
              ? t({ message: "Required", comment: "A BIOS file" })
              : t({ message: "Optional", comment: "A BIOS file" })}{" "}
            ·{" "}
            {file.state === "wrong_hash"
              ? t({ message: "Wrong file", comment: "A BIOS file's state" })
              : file.found === null
                ? t({ message: "Not checked", comment: "A BIOS file's state" })
                : file.found
                  ? t({ message: "Found", comment: "A BIOS file's state" })
                  : t({ message: "Missing", comment: "A BIOS file's state" })}
          </li>
        ))}
      </ul>
    </div>
  );
}
export function PreviewTokens({
  preview,
  pending = false,
  error,
}: {
  preview?: ArgvPreview;
  pending?: boolean;
  error?: string;
}) {
  const { t } = useLingui();
  return (
    <div
      data-cs="command-preview"
      className="rounded-control border border-border bg-surface-base/60 p-3"
    >
      <p className="mb-2 font-mono text-xs tracking-wide text-fg-subtle uppercase">
        <Trans>Resolved command line</Trans>
      </p>
      {pending && (
        <p role="status" className="text-xs text-fg-muted">
          <Trans>Updating preview…</Trans>
        </p>
      )}
      {preview && (
        <>
          <p className="mb-2 break-all text-xs text-fg-subtle">
            <Trans>Sample ROM: {preview.sampleRom}</Trans>
          </p>
          <ol aria-label={t`Command line`} className="flex flex-wrap gap-1.5 font-mono text-xs">
            {[preview.program, ...preview.args].map((token, index) => (
              <li
                // biome-ignore lint/suspicious/noArrayIndexKey: position is the identity of an argv token.
                key={index}
                data-part="token"
                data-program={index === 0 || undefined}
                className="max-w-full break-all rounded-control border border-border bg-surface-overlay px-2 py-1 text-fg-muted data-program:border-accent-alt/40 data-program:text-accent-alt"
              >
                {token === "" ? '""' : token}
              </li>
            ))}
          </ol>
          {preview.cwd && (
            <p className="mt-2 break-all font-mono text-xs text-fg-subtle">
              <Trans>Working directory: {preview.cwd}</Trans>
            </p>
          )}
        </>
      )}
      {(error || preview?.error) && (
        <p role="alert" className="mt-2 text-sm text-danger">
          {error || preview?.error}
        </p>
      )}
    </div>
  );
}
