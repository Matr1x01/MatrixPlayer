import { getCurrentWebview } from '@tauri-apps/api/webview';
import { useEffect, useState } from 'react';

import { Icon } from '@/components/icons/Icon';

/**
 * Shown while files are dragged over the window, so it is clear they will
 * be taken (the core queues whatever is dropped).
 */
export function DropOverlay() {
  const [over, setOver] = useState(false);
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    getCurrentWebview()
      .onDragDropEvent(({ payload }) => setOver(payload.type === 'enter' || payload.type === 'over'))
      .then((u) => (cancelled ? u() : (unlisten = u)))
      .catch(() => undefined);
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);
  if (!over) return null;
  return (
    <div className="drop-overlay" aria-hidden="true">
      <div className="drop-card">
        <Icon name="queue" size={22} />
        <span className="text-[15px] font-semibold">Drop to add to the queue</span>
        <span className="t3 text-[12px]">Audio files and folders</span>
      </div>
    </div>
  );
}
