import { type PluginContext, PluginScope } from "@caskade/sdk";
import { Trans } from "@lingui/react/macro";
import { Component, createRef, type ReactNode } from "react";

interface Props {
  plugin: { id: string; name: string };
  /** The plugin's context, so SDK hooks inside know their plugin. */
  context?: PluginContext;
  children: ReactNode;
}

/** Wraps every contributed component (TECH-PLAN §4.4): a plugin that throws
 * while rendering shows this placeholder and the shell keeps working. Focus
 * that was inside the crashed part lands on the placeholder, so a pad can
 * move on from there (#152). */
export class PluginBoundary extends Component<Props, { failed: boolean }> {
  state = { failed: false };
  private placeholder = createRef<HTMLDivElement>();

  static getDerivedStateFromError() {
    return { failed: true };
  }

  componentDidCatch(error: unknown) {
    console.error(`Plugin ${this.props.plugin.id} failed to render`, error);
  }

  componentDidUpdate(_props: Props, previous: { failed: boolean }) {
    if (!this.state.failed || previous.failed) return;
    const current = document.activeElement;
    if (!current || current === document.body) this.placeholder.current?.focus();
  }

  render() {
    const name = this.props.plugin.name;
    if (this.state.failed)
      return (
        <div
          ref={this.placeholder}
          role="alert"
          // Holds focus after a crash; directions move on from it.
          tabIndex={-1}
          data-cs="plugin-error"
          data-plugin={this.props.plugin.id}
          className="rounded-control border border-dashed border-border-strong px-3 py-2 text-xs text-fg-muted"
        >
          <Trans>{name} couldn't show this part.</Trans>
        </div>
      );
    return (
      <PluginScope.Provider value={this.props.context ?? null}>
        {this.props.children}
      </PluginScope.Provider>
    );
  }
}
