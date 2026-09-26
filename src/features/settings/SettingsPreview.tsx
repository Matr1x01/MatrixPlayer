import { Icon } from '@/components/icons/Icon';
import { Artwork } from '@/components/ui/Artwork';
import { EqIndicator } from '@/components/ui/EqIndicator';
import { formatBadge, formatTime, isHiRes } from '@/features/player/format';
import { usePlayer } from '@/stores/player';

/** Live preview of the current theme, using the real queue when there is one. */
export function SettingsPreview() {
  const track = usePlayer((s) => s.track);
  const queue = usePlayer((s) => s.queue);
  const positionMs = usePlayer((s) => s.positionMs);
  const durationMs = usePlayer((s) => s.durationMs);
  const current = queue.currentIndex ?? 0;
  const rows = queue.entries.slice(Math.max(0, current - 1), Math.max(0, current - 1) + 3);
  const progress = durationMs ? (positionMs / durationMs) * 100 : 39;

  return (
    <aside className="flex flex-col gap-3" aria-label="Live preview">
      <span className="cap pt-[34px]">Preview</span>
      <div className="set-preview">
        <div className="set-preview-head">
          <Artwork artwork={track?.artwork} size={72} title={track?.metadata.album ?? 'MatrixPlayer'} />
          <div className="flex min-w-0 flex-col gap-[3px]">
            <span className="cap" style={{ color: 'var(--ac)' }}>
              Now playing
            </span>
            <span className="el text-[16px] font-bold">{track?.metadata.title ?? 'Track title'}</span>
            <span className="el t2 text-[12px]">{track?.metadata.artist ?? 'Artist'}</span>
          </div>
        </div>
        <div className="flex flex-col gap-2.5 bg-bg2 px-3.5 pb-3.5 pt-2.5">
          <div className="bar">
            <i style={{ width: `${progress}%` }} />
            <span className="knob" style={{ left: `${progress}%`, boxShadow: '0 0 0 3px var(--bg2)' }} />
          </div>
          <div className="flex items-center justify-center gap-4">
            <Icon name="shuffle" size={16} style={{ color: 'var(--ac)' }} />
            <Icon name="prev" size={18} filled />
            <span className="play">
              <Icon name="pause" filled />
            </span>
            <Icon name="next" size={18} filled />
            <Icon name="repeat" size={16} className="t3" />
          </div>
        </div>
        <div className="flex flex-col gap-px border-t border-ln p-1.5">
          {rows.length === 0 && ['Pressure Front', 'Current track', 'Next track'].map((t, i) => <PreviewRow key={t} title={t} n={i + 1} now={i === 1} />)}
          {rows.map((e) => (
            <PreviewRow
              key={e.id}
              title={e.track.metadata.title}
              n={queue.entries.indexOf(e) + 1}
              now={queue.entries.indexOf(e) === current}
              badge={isHiRes(e.track.properties) ? formatBadge(e.track.properties) : undefined}
              duration={formatTime(e.track.properties.durationMs || null)}
            />
          ))}
        </div>
      </div>
      <span className="t3 text-[11.5px]">Changes apply instantly. The accent follows the theme until you pick one.</span>
    </aside>
  );
}

function PreviewRow({ title, n, now, badge, duration }: { title: string; n: number; now: boolean; badge?: string; duration?: string }) {
  return (
    <div className="set-preview-row" style={now ? { background: 'var(--acs)' } : undefined}>
      {now ? <EqIndicator /> : <span className="mono t3 text-[11px]">{n}</span>}
      <span className="el" style={{ color: now ? 'var(--ac)' : 'var(--tx)', fontWeight: now ? 600 : 400 }}>
        {title}
      </span>
      <span className="flex items-center gap-2">
        {badge && <span className="chip hr">{badge}</span>}
        {duration && <span className="mono text-[11px]">{duration}</span>}
      </span>
    </div>
  );
}
