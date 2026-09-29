// Bit-perfect and exclusive playback, as the Audio / DSP view and the
// signal paths describe it: what the core's verdict means, and the warning
// shown before either is switched on.

import { askConfirm } from '@/components/ui/Dialog';
import { khz } from '@/features/player/format';
import type { BitPerfectVerdict } from '@/types/player';

export interface VerdictText {
  /** Short, for chips: "Bit-perfect", "Not bit-perfect". */
  label: string;
  /** What happens to the samples. */
  detail: string;
  ok: boolean;
}

/** Words for the core's verdict. */
export function verdictText(verdict: BitPerfectVerdict): VerdictText {
  switch (verdict.state) {
    case 'off':
      return { label: 'Off', detail: 'Processing as set up', ok: false };
    case 'pending':
      return { label: 'Bit-perfect', detail: 'Checked once playback starts', ok: true };
    case 'yes':
      return verdict.shared
        ? { label: 'Bit-perfect', detail: 'Unchanged through PipeWire, while no other app plays and its volume is at 100%', ok: true }
        : { label: 'Bit-perfect', detail: 'Unchanged, straight to the device', ok: true };
    case 'resampled':
      return { label: 'Not bit-perfect', detail: `Resampled ${khz(verdict.from)} → ${khz(verdict.to)} kHz`, ok: false };
    case 'truncated':
      return { label: 'Not bit-perfect', detail: `${verdict.from}-bit cut to ${verdict.to}-bit`, ok: false };
    case 'remixed':
      return { label: 'Not bit-perfect', detail: `${verdict.from} channels mixed to ${verdict.to}`, ok: false };
    case 'fromFloat':
      return { label: 'Not bit-perfect', detail: 'Lossy file: decoded as float, rounded for the device', ok: false };
  }
}

/**
 * Bit-perfect playback bypasses MatrixPlayer's volume: music plays at full
 * level. Asks first; `device` names the device to be held exclusively.
 */
export function confirmBitPerfect(device?: string): Promise<boolean> {
  const exclusive = device
    ? ` Other apps can't play through ${device} while MatrixPlayer plays; they get it back when you pause.`
    : '';
  return askConfirm({
    title: device ? `Use ${device} exclusively?` : 'Play bit-perfect?',
    body: `MatrixPlayer's volume and all processing (equalizer, ReplayGain, crossfeed) are bypassed, so music plays at full level: set the volume on your DAC or amplifier first.${exclusive}`,
    confirm: 'Turn on',
  });
}
