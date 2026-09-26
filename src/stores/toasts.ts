import { create } from 'zustand';

export type ToastTone = 'info' | 'error';

export interface Toast {
  id: number;
  tone: ToastTone;
  message: string;
  /** Secondary line, e.g. the file an error refers to. */
  detail?: string;
}

interface ToastStore {
  toasts: Toast[];
  dismiss: (id: number) => void;
}

const LIFETIME_MS: Record<ToastTone, number> = { info: 3500, error: 7000 };
const MAX_VISIBLE = 3;
let nextId = 1;

export const useToasts = create<ToastStore>()((set) => ({
  toasts: [],
  dismiss: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
}));

export function showToast(toast: Omit<Toast, 'id'>) {
  const id = nextId++;
  useToasts.setState((s) => ({ toasts: [...s.toasts, { ...toast, id }].slice(-MAX_VISIBLE) }));
  window.setTimeout(() => useToasts.getState().dismiss(id), LIFETIME_MS[toast.tone]);
}

/** Extracts a readable message from a rejected command. */
export function errorMessage(err: unknown): string {
  if (typeof err === 'string') return err;
  if (err && typeof err === 'object' && 'message' in err && typeof err.message === 'string') return err.message;
  return 'Something went wrong';
}

/** Runs a command and reports a failure as a toast instead of throwing. */
export async function run<T>(command: Promise<T>): Promise<T | undefined> {
  try {
    return await command;
  } catch (err) {
    showToast({ tone: 'error', message: errorMessage(err) });
    return undefined;
  }
}
