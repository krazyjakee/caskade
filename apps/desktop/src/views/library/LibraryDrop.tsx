import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { FilePlus2 } from "lucide-react";
import { useState } from "react";
import { useInstallRequests } from "../../api/installRequests";
import { useDropZone } from "../../drop/DropProvider";

/** Files dropped on the library install: a shortcut, a game's executable or
 * folder, an installer or an archive. The core prepares each one (a
 * shortcut's target, an executable's game folder) and the install
 * confirmation opens with it. */
export function useLibraryDrop() {
  const requests = useInstallRequests();
  const [error, setError] = useState<string | null>(null);
  const zone = useDropZone<HTMLDivElement>(
    requests &&
      ((paths) => {
        setError(null);
        for (const path of paths)
          requests
            .requestDrop(path)
            .catch((error: unknown) =>
              setError(error instanceof Error ? error.message : t`Could not add the dropped file.`),
            );
      }),
  );
  return { ...zone, error, clearError: () => setError(null) };
}

/** Shown while files are dragged over the library. */
export function LibraryDropHint() {
  return (
    <div
      data-part="drop-hint"
      className="pointer-events-none fixed inset-x-0 bottom-10 z-40 flex justify-center"
    >
      <p className="flex items-center gap-2 rounded-full border border-accent-strong bg-surface-overlay px-5 py-2 text-sm shadow-lg">
        <FilePlus2 className="size-4 text-accent-alt" />
        <Trans>Drop to install into your library</Trans>
      </p>
    </div>
  );
}
