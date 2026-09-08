import { Component, type ErrorInfo, type ReactNode } from "react";

type Props = {
  children: ReactNode;
  resetKey?: string;
};

type State = {
  error: Error | null;
};

export class PageErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("page render failed", error, info.componentStack);
  }

  componentDidUpdate(prev: Props) {
    if (prev.resetKey !== this.props.resetKey && this.state.error) {
      this.setState({ error: null });
    }
  }

  render() {
    if (this.state.error) {
      return (
        <div className="p-3">
          <h1 className="text-header">This page failed to render</h1>
          <p className="mt-2 font-mono text-label text-negative">{this.state.error.message}</p>
          <button
            type="button"
            className="mt-3 rounded border border-hairline px-2 py-1 text-label hover:bg-surface2"
            onClick={() => this.setState({ error: null })}
          >
            Retry
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}
