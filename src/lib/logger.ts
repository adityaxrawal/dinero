import { invoke } from "@tauri-apps/api/core";

type LogLevel = "debug" | "info" | "warn" | "error";
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
    this.isDev = import.meta.env.DEV ?? true;
  }

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
      // Ignore IPC logging errors to prevent recursive failure loops
    }
  }

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

  public userAction(action: string, targetElement?: string, meta?: unknown) {
    const msg = `User Action: ${action}${targetElement ? ` on [${targetElement}]` : ""}`;
    this.emit("info", msg, meta, "frontend");
  }
}

export const logger = new Logger();
