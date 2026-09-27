import { Icon } from '@/components/icons/Icon';
import { useToasts } from '@/stores/toasts';

/** Transient notices, bottom-left above the player bar (Components · B7). */
export function ToastHost() {
  const toasts = useToasts((s) => s.toasts);
  const dismiss = useToasts((s) => s.dismiss);
  if (toasts.length === 0) return null;
  return (
    <div className="toast-host" aria-live="polite">
      {toasts.map((toast) => (
        <div key={toast.id} className={`toast toast-${toast.tone}`} role={toast.tone === 'error' ? 'alert' : 'status'}>
          <span className="toast-icon">
            <Icon name={toast.tone === 'error' ? 'warn' : 'queue'} size={16} />
          </span>
          <span className="flex min-w-0 flex-col gap-0.5">
            <span>{toast.message}</span>
            {toast.detail && <span className="el mono t3 text-[10.5px]">{toast.detail}</span>}
          </span>
          {toast.action && (
            <button
              type="button"
              className="btn sm ml-auto flex-none"
              onClick={() => {
                dismiss(toast.id);
                toast.action!.run();
              }}
            >
              {toast.action.label}
            </button>
          )}
          <button type="button" className={toast.action ? 'btn gh sm px-1.5' : 'btn gh sm ml-auto px-1.5'} onClick={() => dismiss(toast.id)} aria-label="Dismiss">
            <Icon name="x" size={13} />
          </button>
        </div>
      ))}
    </div>
  );
}
