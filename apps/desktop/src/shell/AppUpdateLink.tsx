import { useLingui } from "@lingui/react/macro";
import { Link } from "@tanstack/react-router";
import { CircleArrowUp } from "lucide-react";
import { useAppUpdate } from "../api/appUpdate";

/** The status bar's offer of a new Caskade, once a check has found one:
 * to Settings → General, where it installs. */
export function AppUpdateLink() {
  const { t } = useLingui();
  const { data: status } = useAppUpdate();
  const version = status?.release?.version;
  if (!status || !version || (status.phase !== "available" && status.phase !== "ready")) {
    return null;
  }
  return (
    <Link
      to="/settings"
      search={{ tab: "general" }}
      data-part="update"
      data-status={status.phase}
      className="flex shrink-0 items-center gap-1.5 rounded-sm text-accent-alt hover:text-fg focus-visible:focus-ring"
    >
      <CircleArrowUp aria-hidden="true" className="size-3.5" />
      {status.phase === "ready"
        ? t`Restart for Caskade v${version}`
        : t`Caskade v${version} available`}
    </Link>
  );
}
