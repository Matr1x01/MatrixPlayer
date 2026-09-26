import { Artwork } from '@/components/ui/Artwork';
import { channelLabel, formatBytes, formatDb, formatKbps, formatTime, khz } from '@/features/player/format';
import { queueApi } from '@/services/ipc';
import { useNavigation } from '@/stores/navigation';
import { usePlayer } from '@/stores/player';
import { run } from '@/stores/toasts';
import type { TrackInfo } from '@/types/player';

import { SignalPath } from './SignalPath';

export type SideTab = 'next' | 'lyrics' | 'file';

const UP_NEXT_LIMIT = 8;

function UpNext() {
  const queue = usePlayer((s) => s.queue);
  const navigate = useNavigation((s) => s.navigate);
  const start = (queue.currentIndex ?? -1) + 1;
  const upcoming = queue.entries.slice(start);
  const shown = upcoming.slice(0, UP_NEXT_LIMIT);

  return (
    <>
      <div className="flex items-center justify-between px-5 pb-2 pt-3.5">
        <span className="cap">
          Up next · {shown.length} of {upcoming.length}
        </span>
        <button type="button" className="text-[12px] font-semibold" style={{ color: 'var(--ac)' }} onClick={() => navigate('queue')}>
          Open queue
        </button>
      </div>
      <div className="flex min-h-0 flex-col overflow-y-auto px-2">
        {shown.length === 0 && <div className="t3 px-3 py-2 text-[12.5px]">Nothing queued after this track.</div>}
        {shown.map((entry) => (
          <div
            key={entry.id}
            className="qrow"
            role="button"
            tabIndex={0}
            onDoubleClick={() => void run(queueApi.playEntry(entry.id))}
            onKeyDown={(e) => e.key === 'Enter' && void run(queueApi.playEntry(entry.id))}
          >
            <Artwork artwork={entry.track.artwork} size={36} radius={3} title={entry.track.metadata.album ?? entry.track.metadata.title} />
            <div className="flex min-w-0 flex-col gap-0.5">
              <span className="el font-medium">{entry.track.metadata.title}</span>
              <span className="el t3 text-[12px]">{entry.track.metadata.artist ?? 'Unknown artist'}</span>
            </div>
            <span className="mono t3 text-[11px]">{formatTime(entry.track.properties.durationMs || null)}</span>
          </div>
        ))}
      </div>
    </>
  );
}

function fileRows(track: TrackInfo): [string, string][] {
  const { properties: p, metadata: m } = track;
  const rg = m.replayGain;
  const gains = [
    rg.trackGain != null && `T ${formatDb(rg.trackGain)}`,
    rg.albumGain != null && `A ${formatDb(rg.albumGain)}`,
    (rg.albumPeak ?? rg.trackPeak) != null && `peak ${(rg.albumPeak ?? rg.trackPeak)!.toFixed(3)}`,
  ].filter(Boolean);
  const art = track.artwork
    ? `${track.artwork.source === 'embedded' ? 'embedded' : 'folder'} art ${track.artwork.width}×${track.artwork.height}`
    : 'no artwork';
  const tags = m.tagFormat ? `${m.tagFormat} · ${m.tagItemCount} fields · ${art}` : `No tags · ${art}`;
  const format = [p.bitDepth && `${p.bitDepth}-bit`, p.sampleRate && `${khz(p.sampleRate)} kHz`, channelLabel(p.channels).toLowerCase()]
    .filter(Boolean)
    .join(' / ');

  const rows: [string, string][] = [
    ['Path', track.displayPath],
    ['Size', formatBytes(p.fileSize)],
    ['Duration', formatTime(p.durationMs || null)],
    ['Codec', `${p.codec}${p.lossless ? ' (lossless)' : ''} · ${p.container}`],
    ['Format', format || '—'],
    ['Bitrate', p.bitrateKbps != null ? formatKbps(p.bitrateKbps) : '—'],
    ['ReplayGain', gains.length ? gains.join(' · ') : 'Not tagged'],
    ['Tags', tags],
  ];
  if (m.derivedFields.length) rows.push(['Derived', `${m.derivedFields.join(', ')} (not in tags)`]);
  if (track.probeError) rows.push(['Warning', track.probeError]);
  return rows;
}

function FileInfo() {
  const track = usePlayer((s) => s.track);
  if (!track) return <div className="t3 px-5 py-4 text-[12.5px]">No track loaded.</div>;
  return (
    <div className="flex flex-col gap-2.5 overflow-y-auto px-5 py-[18px]">
      {fileRows(track).map(([k, v]) => (
        <div key={k} className="grid grid-cols-[96px_minmax(0,1fr)] gap-2.5 text-[12px]">
          <span className="t3">{k}</span>
          <span className={k === 'Path' ? 'mono selectable text-[11px] break-all' : 'mono text-[11px] break-all'}>{v}</span>
        </div>
      ))}
    </div>
  );
}

const TABS: { id: SideTab; label: string }[] = [
  { id: 'next', label: 'Up next' },
  { id: 'lyrics', label: 'Lyrics' },
  { id: 'file', label: 'File' },
];

interface SidePanelProps {
  tab: SideTab;
  onTab: (tab: SideTab) => void;
  /** Drawer state, only relevant when the window is narrow. */
  open: boolean;
}

export function SidePanel({ tab, onTab, open }: SidePanelProps) {
  return (
    <aside className={open ? 'np-side open' : 'np-side'}>
      <div className="tabs px-5" role="tablist">
        {TABS.map((t) => (
          <button key={t.id} type="button" role="tab" aria-selected={tab === t.id} className={tab === t.id ? 'on' : undefined} onClick={() => onTab(t.id)}>
            {t.label}
          </button>
        ))}
      </div>
      <div className="flex min-h-0 flex-1 flex-col">
        {tab === 'next' && <UpNext />}
        {tab === 'lyrics' && (
          <div className="t3 px-6 py-6 text-[12.5px] [text-wrap:pretty]">
            Lyrics are not available yet. Embedded and synced (.lrc) lyrics arrive with the advanced player milestone.
          </div>
        )}
        {tab === 'file' && <FileInfo />}
      </div>
      <SignalPath />
    </aside>
  );
}
