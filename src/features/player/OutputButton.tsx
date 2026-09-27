import { Icon } from '@/components/icons/Icon';
import { useNavigation } from '@/stores/navigation';
import { usePlayer } from '@/stores/player';

import { backendLabel, khz } from './format';

/**
 * The active output and its negotiated rate, e.g. "PipeWire 96k".
 * Opens Audio / DSP, where the full signal path is shown.
 */
export function OutputButton({ compact, framed }: { compact?: boolean; framed?: boolean }) {
  const signal = usePlayer((s) => s.signal);
  const navigate = useNavigation((s) => s.navigate);
  const rate = signal.output ? `${khz(signal.output.sampleRate)}k` : null;
  // Name the device when one is chosen; otherwise the system's default on
  // this backend.
  const label = signal.device?.name ?? backendLabel(signal.backend);
  const tip = signal.device ? `Output: ${signal.device.name} · signal path` : `Output: system default (${backendLabel(signal.backend)}) · signal path`;

  return (
    <button
      type="button"
      className={framed ? 'btn glass' : 'btn gh'}
      style={framed ? undefined : { height: 30, padding: '0 8px', fontWeight: 500, fontSize: 12, gap: 7 }}
      data-tip={tip}
      onClick={() => navigate('dsp')}
    >
      <Icon name="hp" size={framed ? 15 : 16} style={{ color: 'var(--ok)' }} />
      {!compact && (
        <span className="btn-label el" style={{ color: 'var(--tx)', maxWidth: 150 }}>
          {label}
        </span>
      )}
      {rate && <span className="mono t3 text-[10px]">{rate}</span>}
    </button>
  );
}
