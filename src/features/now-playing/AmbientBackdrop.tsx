import { useEffect, useState } from 'react';

import { usePlayer } from '@/stores/player';
import { usePreferences } from '@/stores/preferences';
import type { Rgb } from '@/types/player';

const STRENGTH = { off: 0, subtle: 0.7, immersive: 1.35 } as const;
const FADE_MS = 400;

/** Lifts dark or washed-out artwork colours so the glow stays visible. */
function vivid([r, g, b]: Rgb): string {
  const max = Math.max(r, g, b) / 255;
  const min = Math.min(r, g, b) / 255;
  let h = 0;
  const l = (max + min) / 2;
  const d = max - min;
  const s = d === 0 ? 0 : d / (1 - Math.abs(2 * l - 1));
  if (d !== 0) {
    const [rr, gg, bb] = [r / 255, g / 255, b / 255];
    if (max === rr) h = ((gg - bb) / d) % 6;
    else if (max === gg) h = (bb - rr) / d + 2;
    else h = (rr - gg) / d + 4;
  }
  const hue = Math.round((h * 60 + 360) % 360);
  const sat = Math.round(Math.min(1, s * 1.25) * 100);
  const light = Math.round(Math.min(0.62, Math.max(0.36, l)) * 100);
  return `${hue} ${sat}% ${light}%`;
}

function gradient(palette: [Rgb, Rgb, Rgb]): string {
  const [a, b, c] = palette.map(vivid);
  return [
    `radial-gradient(55% 60% at 22% 38%, hsl(${a} / .30), transparent 70%)`,
    `radial-gradient(45% 55% at 62% 12%, hsl(${b} / .34), transparent 72%)`,
    `radial-gradient(40% 50% at 40% 100%, hsl(${c} / .14), transparent 70%)`,
  ].join(', ');
}

interface Layer {
  key: string;
  background: string;
}

/**
 * Soft light taken from the album artwork behind Now Playing
 * (Foundations · artwork ambient ≤ 30% alpha, 400 ms cross-fade).
 */
export function AmbientBackdrop() {
  const artwork = usePlayer((s) => s.track?.artwork ?? null);
  const ambient = usePreferences((s) => s.prefs.ambient);
  const [layers, setLayers] = useState<Layer[]>([]);

  useEffect(() => {
    const next: Layer | null = artwork ? { key: artwork.id, background: gradient(artwork.palette) } : null;
    setLayers((current) => {
      if (current.at(-1)?.key === next?.key) return current;
      return next ? [...current.slice(-1), next] : [];
    });
    const timer = window.setTimeout(() => setLayers((current) => current.slice(-1)), FADE_MS);
    return () => window.clearTimeout(timer);
  }, [artwork]);

  if (ambient === 'off' || layers.length === 0) return null;
  return (
    <div className="np-ambient" style={{ opacity: STRENGTH[ambient] }} aria-hidden="true">
      {layers.map((layer) => (
        <div key={layer.key} className="np-ambient-layer" style={{ background: layer.background }} />
      ))}
    </div>
  );
}
