import { Button } from "@caskade/ui";
import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { type ErrorComponentProps, useRouter } from "@tanstack/react-router";
import { Component, type ReactNode, useRef } from "react";
import { errorMessage } from "../api/reply";
import { useDefaultFocus } from "../input";
import { useAppWindow } from "./appWindow";
import { WindowControls } from "./WindowControls";

/** A view that throws while rendering shows this in its place; the sidebar,
 * topbar and status bar keep working, so the rest of the app stays usable. */
export function ViewError({ error, reset }: ErrorComponentProps) {
  const router = useRouter();
  const retry = useRef<HTMLButtonElement>(null);
  useDefaultFocus(retry);
  return (
    <div data-cs="view-error" role="alert" className="grid flex-1 place-items-center">
      <div className="max-w-md text-center">
        <h1 data-part="title" className="font-display text-2xl font-semibold">
          <Trans>This view couldn't be shown</Trans>
        </h1>
        <p data-part="message" className="mt-2 text-sm break-words text-fg-muted">
          {errorMessage(error)}
        </p>
        <div data-part="actions" className="mt-5 flex justify-center gap-2">
          <Button
            ref={retry}
            variant="primary"
            data-focus-default
            onPress={() => {
              reset();
              void router.invalidate();
            }}
          >
            <Trans>Try again</Trans>
          </Button>
          <Button
            variant="secondary"
            onPress={() => void router.navigate({ to: "/library", search: { filter: "all" } })}
          >
            <Trans>Open the library</Trans>
          </Button>
        </div>
      </div>
    </div>
  );
}

/** Replaces the whole window when the shell itself fails. The window has no
 * native title bar, so this keeps the window controls. */
export function AppCrash({ error }: { error: unknown }) {
  const appWindow = useAppWindow();
  return (
    <div data-cs="view-error" role="alert" className="flex h-full flex-col">
      {appWindow && appWindow.frame === "custom" && (
        <div className="flex justify-end p-2">
          <WindowControls window={appWindow} />
        </div>
      )}
      <div className="grid flex-1 place-items-center">
        <div className="max-w-md text-center">
          <h1 data-part="title" className="font-display text-2xl font-semibold">
            <Trans>Caskade ran into a problem</Trans>
          </h1>
          <p data-part="message" className="mt-2 text-sm break-words text-fg-muted">
            {errorMessage(error)}
          </p>
          <div data-part="actions" className="mt-5 flex justify-center">
            <Button variant="primary" autoFocus onPress={() => window.location.reload()}>
              <Trans>Reload</Trans>
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}

/** The last line: catches what the router's boundaries can't, such as the
 * providers around it. */
export class AppErrorBoundary extends Component<{ children: ReactNode }, { error: unknown }> {
  state: { error: unknown } = { error: null };

  static getDerivedStateFromError(error: unknown) {
    return { error: error ?? new Error(t`Unknown error`) };
  }

  componentDidCatch(error: unknown) {
    console.error("Caskade failed to render", error);
  }

  render() {
    if (this.state.error) return <AppCrash error={this.state.error} />;
    return this.props.children;
  }
}
