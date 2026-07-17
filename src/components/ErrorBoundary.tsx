import { Component, ReactNode } from 'react';
import { AlertTriangle, RefreshCw, FileText } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { API } from '@/lib/ipc';

interface Props {
  children: ReactNode;
}

interface State {
  hasError: boolean;
  error: Error | null;
  isExporting: boolean;
  exportedPath: string | null;
}

/**
 * TASK-FE-018 (Doc 30): "catching render-time exceptions with a friendly
 * 'Something went wrong' screen (a 'Reload' action, an option to export a
 * diagnostic bundle) rather than a raw stack trace." Previously showed
 * `error.message` (often a raw, technical string) with only a "Return to
 * Dashboard" action that just reset local boundary state via `setState` --
 * for a genuine render crash the underlying app/query state that caused it
 * is usually still corrupted, so a full reload is the more reliable
 * recovery path than hoping a re-render alone fixes it.
 */
export class ErrorBoundary extends Component<Props, State> {
  public state: State = {
    hasError: false,
    error: null,
    isExporting: false,
    exportedPath: null,
  };

  public static getDerivedStateFromError(error: Error): Partial<State> {
    return { hasError: true, error };
  }

  public componentDidCatch(error: Error, errorInfo: React.ErrorInfo) {
    console.error('Uncaught error:', error, errorInfo);
  }

  private handleReload = () => {
    window.location.reload();
  };

  private handleExportDiagnosticBundle = async () => {
    this.setState({ isExporting: true, exportedPath: null });
    try {
      const result = await API.support.exportLogs();
      this.setState({ exportedPath: result.file_path });
    } catch (err) {
      console.error('Failed to export diagnostic bundle:', err);
    } finally {
      this.setState({ isExporting: false });
    }
  };

  public render() {
    if (this.state.hasError) {
      return (
        <div
          role="alert"
          className="flex flex-col items-center justify-center h-screen bg-background text-foreground p-8"
        >
          <div className="max-w-md w-full bg-card border border-border rounded-lg p-6 flex flex-col items-center text-center space-y-4">
            <div className="h-12 w-12 rounded-full bg-destructive/20 flex items-center justify-center">
              <AlertTriangle className="text-red-700 w-6 h-6" aria-hidden="true" />
            </div>
            <h2 className="text-xl font-semibold">Something went wrong</h2>
            <p className="text-sm text-muted-foreground">
              Dinero ran into an unexpected problem. Reloading usually fixes this.
            </p>
            <div className="flex flex-col gap-2 w-full">
              <Button variant="default" onClick={this.handleReload} aria-label="Reload the app">
                <RefreshCw className="w-4 h-4 mr-2" aria-hidden="true" />
                Reload
              </Button>
              <Button
                variant="outline"
                onClick={this.handleExportDiagnosticBundle}
                disabled={this.state.isExporting}
                aria-label="Export diagnostic bundle"
              >
                <FileText className="w-4 h-4 mr-2" aria-hidden="true" />
                {this.state.isExporting ? 'Exporting…' : 'Export Diagnostic Bundle'}
              </Button>
            </div>
            {this.state.exportedPath && (
              <p className="text-xs text-muted-foreground">
                Saved locally to: {this.state.exportedPath}
              </p>
            )}
          </div>
        </div>
      );
    }

    return this.props.children;
  }
}
