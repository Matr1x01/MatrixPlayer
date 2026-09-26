import { channelLabel, codecLabel, formatDb, formatKbps, formatRate } from '@/features/player/format';
import type { TrackInfo } from '@/types/player';

/** Codec, depth, rate, bitrate, channels and ReplayGain, as in the design. */
export function TrackChips({ track }: { track: TrackInfo }) {
  const { properties: p, metadata } = track;
  const deep = (p.bitDepth ?? 0) >= 24;
  const fast = (p.sampleRate ?? 0) > 48000;
  const gain = metadata.replayGain.albumGain ?? metadata.replayGain.trackGain;

  return (
    <div className="flex flex-wrap gap-1.5">
      <span className={deep || fast ? 'chip hr' : 'chip'}>{codecLabel(p)}</span>
      {p.bitDepth != null && <span className={deep ? 'chip hr' : 'chip'}>{p.bitDepth}-bit</span>}
      {p.sampleRate != null && <span className={fast ? 'chip hr' : 'chip'}>{formatRate(p.sampleRate)}</span>}
      {p.bitrateKbps != null && <span className="chip">{formatKbps(p.bitrateKbps)}</span>}
      {p.channels != null && <span className="chip">{channelLabel(p.channels)}</span>}
      {gain != null && <span className="chip">RG {formatDb(gain)}</span>}
    </div>
  );
}
