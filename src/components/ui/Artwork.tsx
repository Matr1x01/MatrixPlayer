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

/** Images larger than this (in CSS px) load the full-size original. */
const THUMB_MAX_PX = 180;

export function artworkUrl(id: string, thumb = false): string {
  return convertFileSrc(thumb ? `${id}/thumb` : id, 'art');
}

export function monogram(text: string): string {
  const letters = text
    .split(/\s+/)
    .filter((word) => /^\p{L}/u.test(word))
    .slice(0, 2)
    .map((word) => word[0]!.toUpperCase());
  return letters.join('') || '?';
}

export function fallbackColors(text: string): [string, string, string] {
  let h = 0;
  for (let i = 0; i < text.length; i++) h = (h * 31 + text.charCodeAt(i)) | 0;
  return FALLBACKS[Math.abs(h) % FALLBACKS.length]!;
}

interface ArtworkProps {
  /** Full artwork info (player tracks) … */
  artwork?: ArtworkInfo | null;
  /** … or just an id (library rows). */
  artworkId?: string | null;
  /** Square size in px; omit to fill the parent width. */
  size?: number;
  radius?: number;
  /** Album or track name for the fallback tile. */
  title?: string;
  /** Load the full-size image even when small (Now Playing, album hero). */
  full?: boolean;
  className?: string;
  style?: CSSProperties;
  children?: ReactNode;
}

/**
 * Album art, or the design's monogram tile when there is none. Images fade
 * in once decoded, so grids fill in smoothly instead of popping.
 */
export function Artwork({ artwork, artworkId, size, radius, title = '', full, className, style, children }: ArtworkProps) {
  const id = artwork?.id ?? artworkId ?? null;
  const [failedId, setFailedId] = useState<string | null>(null);
  const [loadedId, setLoadedId] = useState<string | null>(null);
  const box: CSSProperties = size ? { width: size, height: size } : { width: '100%', aspectRatio: '1 / 1' };
  const shape: CSSProperties = { ...box, borderRadius: radius, ...style };

  if (id && failedId !== id) {
    const thumb = !full && size != null && size <= THUMB_MAX_PX;
    // Grid tiles have no fixed size but are small: thumbnails there too.
    const useThumb = thumb || (!full && size == null);
    return (
      <span className={['art art-loading', loadedId === id && 'art-ready', className].filter(Boolean).join(' ')} style={shape}>
        <img
          key={id}
          className="art-img"
          src={artworkUrl(id, useThumb)}
          alt=""
          decoding="async"
          loading="lazy"
          draggable={false}
          onLoad={() => setLoadedId(id)}
          onError={() => setFailedId(id)}
        />
        {children}
      </span>
    );
  }

  const [from, to, fg] = fallbackColors(title);
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
