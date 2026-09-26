import { getCurrentWindow } from '@tauri-apps/api/window';

import { Icon } from '@/components/icons/Icon';

/** Minimise / maximise / close, drawn by the app (the window is undecorated). */
export function WindowControls() {
  const win = getCurrentWindow();
  return (
    <div className="flex items-center gap-1.5">
      <button type="button" className="wc" aria-label="Minimize" onClick={() => void win.minimize()}>
        <Icon name="min" size={10} strokeWidth={3} />
      </button>
      <button type="button" className="wc" aria-label="Maximize" onClick={() => void win.toggleMaximize()}>
        <Icon name="max" size={10} strokeWidth={2.6} />
      </button>
      <button type="button" className="wc wc-close" aria-label="Close" onClick={() => void win.close()}>
        <Icon name="x" size={10} strokeWidth={2.8} />
      </button>
    </div>
  );
}
