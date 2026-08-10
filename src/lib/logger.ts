/**
 * Frontend logging façade that forwards structured events to the Rust backend.
 *
 * Every log takes two paths. In development it is also printed to the webview
 * console for immediate feedback; in all builds it is shipped over IPC to the
 * backend, which owns the persistent log files. That second path is the reason
 * this exists at all -- a packaged desktop app has no console anyone will read,
 * so diagnostics have to reach disk to be useful after the fact.
 *
 * Beyond the four severity methods, the class exposes purpose-built helpers for
 * the event shapes this app emits repeatedly -- IPC calls, outbound HTTP, and
 * user actions -- so those are recorded with consistent wording and structure
 * rather than being formatted differently at each call site.
 */
import { invoke } from "@tauri-apps/api/core";

type LogLevel = "debug" | "info" | "warn" | "error";

// Routing categories on the backend side; each becomes a log target there.

type LogCategory =
  | "frontend"
  | "api_calls"
  | "network"
  | "user_action"
  | "system";

interface LogPayload {
  target?: string;
  level: LogLevel;
  message: string;
  data?: string | null;
}

class Logger {
  private isDev: boolean;

  constructor() {
    // Defaults to true when the flag is absent so that a non-Vite context (a
    // bare test runner, for instance) still gets console output rather than
    // silently logging nowhere.
    this.isDev = import.meta.env.DEV ?? true;
  }

  /**
   * Shared path for every log call: console in development, IPC always.
   *
   * Non-string data is serialised to JSON here rather than at the call sites,
   * so the backend always receives a string or null and callers can pass
   * whatever structured context is convenient.
   */
  private async emit(
    level: LogLevel,
    message: string,
    data?: unknown,
    target: LogCategory | string = "frontend",
  ) {
    const serializedData =
      data !== undefined
        ? typeof data === "string"
          ? data
          : JSON.stringify(data)
        : null;

    // Development console mirror. The level is dispatched to the matching
    // console method rather than always using log, so the browser's own
    // filtering and stack-trace capture behave correctly per severity.
    if (this.isDev) {
      const prefix = `[${target.toUpperCase()}]`;
      switch (level) {
        case "debug":
          console.debug(prefix, message, data ?? "");
          break;
        case "info":
          console.log(prefix, message, data ?? "");
          break;
        case "warn":
          console.warn(prefix, message, data ?? "");
          break;
        case "error":
          console.error(prefix, message, data ?? "");
          break;
      }
    }

    try {
      const payload: LogPayload = {
        target,
        level,
        message,
        data: serializedData,
      };
      await invoke(
        "log_frontend_event",
        payload as unknown as Record<string, unknown>,
      );
    } catch {
      // Swallowed deliberately. This is the logging path itself, so reporting a
      // failure here would attempt another log and recurse; a lost log line is
      // strictly preferable to an infinite failure loop.
    }
  }

  // The four severity entry points. All are thin wrappers over emit, differing
  // only in level, and all are fire-and-forget -- callers never await a log.
  public debug(
    message: string,
    data?: unknown,
    target: LogCategory | string = "frontend",
  ) {
    this.emit("debug", message, data, target);
  }

  public info(
    message: string,
    data?: unknown,
    target: LogCategory | string = "frontend",
  ) {
    this.emit("info", message, data, target);
  }

  public warn(
    message: string,
    data?: unknown,
    target: LogCategory | string = "frontend",
  ) {
    this.emit("warn", message, data, target);
  }

  public error(
    message: string,
    data?: unknown,
    target: LogCategory | string = "frontend",
  ) {
    this.emit("error", message, data, target);
  }

  /**
   * Record one IPC command round-trip, including its duration.
   *
   * Called automatically by the IPC wrapper for every backend command, which is
   * what makes the api_calls log a complete timeline of frontend/backend
   * traffic. Severity follows the outcome, so failures surface as errors
   * without the caller having to choose a level.
   */
  public apiCall(
    command: string,
    args: unknown,
    durationMs: number,
    success: boolean,
    errorDetails?: unknown,
  ) {
    const statusStr = success ? "SUCCESS" : "FAILED";
    const msg = `IPC Command: ${command} [${statusStr}] (${durationMs.toFixed(1)}ms)`;
    const details = {
      command,
      args,
      durationMs,
      success,
      error: errorDetails,
    };
    this.emit(success ? "info" : "error", msg, details, "api_calls");
  }

  /**
   * Record an outbound HTTP request made from the frontend.
   *
   * A null status means the request never completed at all (DNS failure,
   * timeout, refused connection) and is rendered as "ERR"; that case and any
   * 4xx/5xx response are both logged at error level.
   */
  public network(
    url: string,
    method: string,
    status: number | null,
    latencyMs: number,
    errorDetails?: unknown,
  ) {
    const msg = `Outbound HTTP ${method} ${url} - Status: ${status ?? "ERR"} (${latencyMs.toFixed(1)}ms)`;
    const details = { url, method, status, latencyMs, error: errorDetails };
    this.emit(
      status && status < 400 ? "info" : "error",
      msg,
      details,
      "network",
    );
  }

  /**
   * Record a deliberate user interaction.
   *
   * Provides the "what was the user doing?" context that makes an adjacent
   * error log interpretable when reconstructing a session after the fact.
   */
  public userAction(action: string, targetElement?: string, meta?: unknown) {
    const msg = `User Action: ${action}${targetElement ? ` on [${targetElement}]` : ""}`;
    this.emit("info", msg, meta, "frontend");
  }
}

// Single shared instance -- logging is process-wide, and a second logger would
// only duplicate the dev-mode detection with no benefit.
export const logger = new Logger();
