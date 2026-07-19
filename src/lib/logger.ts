import { invoke } from '@tauri-apps/api/core';

const logFrontendEvent = (level: string, message: string, data?: any) => {
  try {
    invoke('log_frontend_event', {
      level,
      message,
      data: data ? data : null,
    }).catch(() => { /* Ignore background IPC failures */ });
  } catch (e) {
    // Ignore errors if run outside Tauri
  }
};
export const initLogger = () => {
  const originalLog = console.log;
  const originalWarn = console.warn;
  const originalError = console.error;
  const originalDebug = console.debug;
  const originalTrace = console.trace;

  const formatArgs = (args: any[]) => {
    return args
      .map(a => {
        if (a instanceof Error) return a.toString();
        return typeof a === 'object' ? JSON.stringify(a) : String(a);
      })
      .join(' ');
  };

  console.log = (...args: any[]) => {
    originalLog(...args);
    logFrontendEvent('info', formatArgs(args));
  };

  console.warn = (...args: any[]) => {
    originalWarn(...args);
    logFrontendEvent('warn', formatArgs(args));
  };

  console.error = (...args: any[]) => {
    originalError(...args);
    const msg = formatArgs(args);
    if (msg.includes('invalid args `event` for command `listen`') || msg.includes("invalid args 'event' for command 'listen'")) {
      logFrontendEvent('error', msg + '\nSTACK: ' + new Error().stack);
    } else {
      logFrontendEvent('error', msg);
    }
  };

  console.debug = (...args: any[]) => {
    originalDebug(...args);
    logFrontendEvent('debug', formatArgs(args));
  };

  console.trace = (...args: any[]) => {
    originalTrace(...args);
    logFrontendEvent('trace', formatArgs(args));
  };
};
