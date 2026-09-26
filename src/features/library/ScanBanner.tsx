import { useEffect, useRef, useState } from 'react';

import { isScanning, useLibrary } from '@/stores/library';
import type { ScanStatus } from '@/types/library';

import { count } from './format';

/** Remaining time from the reading rate so far, once it is meaningful. */
function useEta(scan: ScanStatus): string | null {
  const started = useRef<{ at: number; processed: number } | null>(null);
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    if (scan.phase !== 'reading') {
      started.current = null;
      return;
    }
    started.current ??= { at: Date.now(), processed: scan.processed };
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [scan.phase, scan.processed]);
  const s = started.current;
  if (!s || scan.processed - s.processed < 20) return null;
  const rate = (scan.processed - s.processed) / ((now - s.at) / 1000);
  const left = (scan.total - scan.processed) / Math.max(rate, 0.1);
  if (left < 5) return 'almost done';
  if (left < 90) return `~${Math.ceil(left / 5) * 5} s left`;
  return `~${Math.round(left / 60)} min left`;
}

function folderOf(path: string | null): string {
  if (!path) return '';
  const cut = path.lastIndexOf('/');
  const dir = cut > 0 ? path.slice(0, cut + 1) : path;
  const home = dir.match(/^\/home\/[^/]+\//)?.[0];
  return home ? `~/${dir.slice(home.length)}` : dir;
}

/** Scan progress (Empty & loading states · 02), shown above library views. */
export function ScanBanner() {
  const scan = useLibrary((s) => s.scan);
  const eta = useEta(scan);
  if (!isScanning(scan)) return null;

  const reading = scan.phase === 'reading' && scan.total > 0;
  const progress = reading ? scan.processed / scan.total : scan.phase === 'finishing' ? 1 : 0;
  const where = scan.phase === 'discovering' ? (scan.current ?? scan.root) : folderOf(scan.current);
  return (
    <div className="scan-banner" role="status" aria-live="polite">
      <div className="flex items-baseline justify-between gap-4">
        <span className="el font-semibold">
          {scan.phase === 'discovering' ? 'Looking for music in ' : scan.phase === 'finishing' ? 'Finishing up ' : 'Scanning '}
          <span className="mono t2 text-[12px]">{where}</span>
        </span>
        <span className="mono t2 flex-none text-[11px]">
          {reading
            ? `${count(scan.processed)} / ${count(scan.total)}${eta ? ` · ${eta}` : ''}`
            : scan.phase === 'discovering'
              ? `${count(scan.found)} files found`
              : 'updating albums'}
        </span>
      </div>
      <div className={reading || scan.phase === 'finishing' ? 'bar' : 'bar indeterminate'}>
        <i style={{ width: `${Math.round(progress * 100)}%` }} />
      </div>
      <div className="mono t3 flex gap-4 text-[10.5px]">
        {scan.added > 0 && <span>New ✓ {count(scan.added)}</span>}
        {scan.updated > 0 && <span>Updated {count(scan.updated)}</span>}
        {scan.removed > 0 && <span>Removed {count(scan.removed)}</span>}
        {scan.unreadable > 0 && <span style={{ color: 'var(--warn)' }}>Unreadable folders {scan.unreadable}</span>}
        <span className="t2 ml-auto">Library stays usable while scanning</span>
      </div>
    </div>
  );
}
