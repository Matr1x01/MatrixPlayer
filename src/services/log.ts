// Sends what goes wrong in the interface to MatrixPlayer's log files, next
// to the core's own records: uncaught errors, rejected promises nobody
// handled, console errors and warnings, and the errors shown to the user.
// The webview's console is gone once the window closes; the log is not.

import { systemApi } from '@/services/ipc';

type Level = 'error' | 'warn' | 'info';

/** Most messages sent in a minute; past that they are only counted, so a
 * render loop that throws cannot flood the log. */
const BUDGET = 30;
const WINDOW_MS = 60_000;

let windowStart = 0;
let sent = 0;
let dropped = 0;

/** The console as it was, for reporting without logging twice. */
const original = { error: console.error.bind(console), warn: console.warn.bind(console) };

function send(level: Level, message: string, detail?: string) {
  try {
    systemApi.log(level, message, detail).catch(() => {});
  } catch {
    // No core to send it to (a plain browser); nowhere left to report it.
  }
}

/** Writes a line to MatrixPlayer's log. Never throws. */
export function logToCore(level: Level, message: string, detail?: string) {
  const now = Date.now();
  if (now - windowStart > WINDOW_MS) {
    if (dropped > 0) send('warn', `${dropped} more interface messages were not logged`);
    windowStart = now;
    sent = 0;
    dropped = 0;
  }
  if (sent >= BUDGET) {
    dropped += 1;
    return;
  }
  sent += 1;
  send(level, message, detail);
}

/** A readable one-line description of anything thrown or logged. */
export function describe(value: unknown): string {
  if (value instanceof Error) return `${value.name}: ${value.message}`;
  if (typeof value === 'string') return value;
  if (value && typeof value === 'object' && 'message' in value && typeof value.message === 'string') {
    return 'kind' in value && typeof value.kind === 'string' ? `${value.kind}: ${value.message}` : value.message;
  }
  try {
    return JSON.stringify(value) ?? String(value);
  } catch {
    return String(value);
  }
}

function stackOf(value: unknown): string | undefined {
  return value instanceof Error ? value.stack : undefined;
}

/** A render error React could not recover from: the window goes blank. */
export function logUncaught(error: unknown, componentStack?: string) {
  original.error(error);
  const detail = [stackOf(error), componentStack && `in components:${componentStack}`].filter(Boolean).join('\n');
  logToCore('error', `the interface crashed: ${describe(error)}`, detail || undefined);
}

/** Starts sending the interface's errors to the log. Call once, first. */
export function captureInterfaceErrors() {
  window.addEventListener('error', (event) => {
    const where = event.filename ? `${event.filename}:${event.lineno}:${event.colno}` : undefined;
    logToCore('error', `uncaught: ${event.error ? describe(event.error) : event.message}`, stackOf(event.error) ?? where);
  });
  window.addEventListener('unhandledrejection', (event) => {
    logToCore('error', `unhandled rejection: ${describe(event.reason)}`, stackOf(event.reason));
  });
  for (const level of ['error', 'warn'] as const) {
    console[level] = (...args: unknown[]) => {
      original[level](...args);
      logToCore(level, `console: ${args.map(describe).join(' ')}`, args.map(stackOf).find(Boolean));
    };
  }
}
