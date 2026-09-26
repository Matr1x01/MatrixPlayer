import { convertFileSrc } from '@tauri-apps/api/core';
import { useState, type CSSProperties, type ReactNode } from 'react';

import { Icon } from '@/components/icons/Icon';
import type { ArtworkInfo } from '@/types/player';

/** Fallback palettes from the design's "No artwork" state. */
const FALLBACKS: [string, string, string][] = [
  ['#2b3242', '#1b2029', '#c6cfdf'],
  ['#3a3326', '#231e16', '#e3d3b0'],
  ['#2f3a33', '#1b221d', '#c3d6c8'],
  ['#3b2a33', '#22171d', '#e3c3d2'],
  ['#2a2a2e', '#1a1a1d', '#a9a7a1'],
];

export function artworkUrl(artwork: ArtworkInfo): string {
  return convertFileSrc(artwork.id, 'art');
}

function monogram(text: string): string {
  const letters = text
    .split(/\s+/)
    .filter((word) => /^\p{L}/u.test(word))
    .slice(0, 2)
    .map((word) => word[0]!.toUpperCase());
  return letters.join('') || '?';
}

function hash(text: string): number {
  let h = 0;
  for (let i = 0; i < text.length; i++) h = (h * 31 + text.charCodeAt(i)) | 0;
  return Math.abs(h);
}

interface ArtworkProps {
  artwork: ArtworkInfo | null | undefined;
  /** Square size in px; omit to fill the parent width. */
  size?: number;
  radius?: number;
  /** Album or track name for the fallback tile. */
  title?: string;
  className?: string;
  style?: CSSProperties;
  children?: ReactNode;
}

/** Album art, or the design's monogram tile when a track has none. */
export function Artwork({ artwork, size, radius, title = '', className, style, children }: ArtworkProps) {
  const [failedId, setFailedId] = useState<string | null>(null);
  const box: CSSProperties = size ? { width: size, height: size } : { width: '100%', aspectRatio: '1 / 1' };
  const shape: CSSProperties = { ...box, borderRadius: radius, ...style };

  if (artwork && failedId !== artwork.id) {
    return (
      <span className={['art', className].filter(Boolean).join(' ')} style={shape}>
        <img
          className="art-img"
          src={artworkUrl(artwork)}
          alt=""
          decoding="async"
          draggable={false}
          onError={() => setFailedId(artwork.id)}
        />
        {children}
      </span>
    );
  }

  const [from, to, fg] = FALLBACKS[hash(title) % FALLBACKS.length]!;
  const compact = size != null && size < 96;
  return (
    <span
      className={['art art-fallback', compact && 'compact', className].filter(Boolean).join(' ')}
      style={{ ...shape, background: `linear-gradient(160deg, ${from}, ${to})`, color: fg }}
      role="img"
      aria-label={title ? `No artwork for ${title}` : 'No artwork'}
    >
      {compact ? (
        <span className="mg" style={{ fontSize: Math.max(10, Math.round((size ?? 32) * 0.36)) }}>
          {monogram(title)}
        </span>
      ) : (
        <>
          <Icon name="album" size={16} style={{ opacity: 0.6 }} />
          <span className="flex min-w-0 flex-col gap-1">
            <span className="mg">{monogram(title)}</span>
            {title && <span className="ti el">{title}</span>}
          </span>
        </>
      )}
      {children}
    </span>
  );
}
