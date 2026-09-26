import { showDialog } from '@/components/ui/Dialog';
import { Icon } from '@/components/icons/Icon';
import { channelLabel, formatKbps, khz } from '@/features/player/format';
import { navigate } from '@/stores/navigation';
import type { TrackRow } from '@/types/library';

import { formatDate, formatSize, trackBadge } from './format';

function Kv({ k, v, tone }: { k: string; v: string; tone?: string }) {
  return (
    <div className="kv">
      <span>{k}</span>
      <span className="el selectable" style={tone ? { color: tone } : undefined}>
        {v}
      </span>
    </div>
  );
}

/** The design's "Audio information" card, for one library track. */
export function showTrackInfo(track: TrackRow) {
  const hiRes = track.lossless && ((track.bitDepth ?? 0) > 16 || (track.sampleRate ?? 0) > 48000);
  return showDialog(
    'File information',
    (close) => (
      <div className="flex flex-col">
        <div className="info-hero">
          <div className="flex items-center justify-between">
            <span className="cap">Audio information</span>
            <span className={hiRes ? 'chip hr' : 'chip'}>{hiRes ? 'HI-RES LOSSLESS' : track.lossless ? 'LOSSLESS' : 'LOSSY'}</span>
          </div>
          <div className="flex items-baseline gap-2">
            <span className="mono text-[28px] font-semibold tracking-[-0.02em]">
              {track.lossless && track.bitDepth && track.sampleRate ? (
                <>
                  {track.bitDepth}
                  <span className="t3 text-[17px]">/</span>
                  {khz(track.sampleRate)}
                </>
              ) : (
                trackBadge(track)
              )}
            </span>
            <span className="t2 text-[12px]">
              {track.codec} · {channelLabel(track.channels).toLowerCase()}
            </span>
          </div>
        </div>
        <div className="flex flex-col px-1 py-2">
          <Kv k="Title" v={track.title} />
          <Kv k="File" v={track.fileName} />
          <Kv k="Codec" v={`${track.codec}${track.lossless ? ' (lossless)' : ''}`} />
          <Kv k="Sample rate" v={track.sampleRate ? `${track.sampleRate.toLocaleString('en-US')} Hz` : '—'} />
          <Kv k="Bit depth" v={track.bitDepth ? `${track.bitDepth}-bit` : '—'} />
          <Kv k="Bitrate" v={track.bitrateKbps ? formatKbps(track.bitrateKbps) : '—'} />
          <Kv k="Channels" v={track.channels ? `${track.channels} · ${channelLabel(track.channels).toLowerCase()}` : '—'} />
          <Kv k="Size" v={formatSize(track.fileSize)} />
          <Kv k="Added" v={formatDate(track.addedAt)} />
          <Kv k="Plays" v={track.playCount ? `${track.playCount} · last ${formatDate(track.lastPlayedAt, true)}` : 'Never played'} />
          {!track.available && <Kv k="Status" v="Drive offline · file unavailable" tone="var(--warn)" />}
          {track.titleDerived && <Kv k="Tags" v="Untagged · title from file name" tone="var(--warn)" />}
        </div>
        <button
          type="button"
          className="info-link"
          onClick={() => {
            close();
            navigate('folders', track.folderId);
          }}
        >
          Browse folder
          <Icon name="chevR" size={14} />
        </button>
      </div>
    ),
    360,
  );
}
