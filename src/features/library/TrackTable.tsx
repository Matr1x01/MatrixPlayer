import { memo, useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent, type MouseEvent, type ReactNode } from 'react';

import { Icon } from '@/components/icons/Icon';
import { Artwork } from '@/components/ui/Artwork';
import { contextMenu, type MenuItem } from '@/components/ui/ContextMenu';
import { EqIndicator } from '@/components/ui/EqIndicator';
import { formatKbps, formatTime, khz } from '@/features/player/format';
import { useTrackRows } from '@/hooks/useTrackRows';
import { useVirtualList } from '@/hooks/useVirtual';
import { setFavorite, useLibrary } from '@/stores/library';
import { navigate } from '@/stores/navigation';
import { usePlayer } from '@/stores/player';
import type { TrackRow, TrackSort } from '@/types/library';

import { playTracks } from './actions';
import { badgeClass, formatLength, formatSize, trackBadge } from './format';
import { showTrackInfo } from './TrackInfoDialog';
import { trackMenu } from './trackMenu';

export type ColumnId =
  | 'num'
  | 'index'
  | 'art'
  | 'title'
  | 'titleArtist'
  | 'artist'
  | 'album'
  | 'year'
  | 'genre'
  | 'format'
  | 'rate'
  | 'bits'
  | 'rateBits'
  | 'bitrate'
  | 'size'
  | 'time'
  | 'heart'
  | 'plays'
  | 'added'
  | 'more';

interface Column {
  label: string;
  width: string;
  right?: boolean;
  sort?: TrackSort;
  mono?: boolean;
}

const COLUMNS: Record<ColumnId, Column> = {
  num: { label: '#', width: '30px', right: true, mono: true },
  index: { label: '#', width: '30px', right: true, mono: true },
  art: { label: '', width: '26px' },
  title: { label: 'Title', width: 'minmax(0,1.45fr)', sort: 'title' },
  titleArtist: { label: 'Title', width: 'minmax(0,1.6fr)', sort: 'title' },
  artist: { label: 'Artist', width: 'minmax(0,1fr)', sort: 'artist' },
  album: { label: 'Album', width: 'minmax(0,1.15fr)', sort: 'album' },
  year: { label: 'Year', width: '40px', sort: 'year', mono: true },
  genre: { label: 'Genre', width: '84px', sort: 'genre' },
  format: { label: 'Format', width: '84px', sort: 'format' },
  rate: { label: 'Rate', width: '52px', sort: 'sampleRate', mono: true },
  bits: { label: 'Bits', width: '30px', sort: 'bitDepth', mono: true },
  rateBits: { label: 'Rate / bits', width: '92px', sort: 'sampleRate', mono: true },
  bitrate: { label: 'Bitrate', width: '84px', right: true, mono: true },
  size: { label: 'Size', width: '64px', right: true, sort: 'size', mono: true },
  time: { label: 'Time', width: '46px', right: true, sort: 'duration', mono: true },
  heart: { label: '', width: '22px' },
  plays: { label: 'Plays', width: '44px', right: true, sort: 'plays', mono: true },
  added: { label: 'Added', width: '82px', right: true, sort: 'added', mono: true },
  more: { label: '', width: '28px' },
};

export const SONG_COLUMNS: ColumnId[] = ['num', 'art', 'title', 'artist', 'album', 'year', 'genre', 'format', 'rate', 'bits', 'size', 'time'];
export const ALBUM_COLUMNS: ColumnId[] = ['num', 'title', 'heart', 'format', 'rateBits', 'bitrate', 'size', 'time', 'more'];
export const LIST_COLUMNS: ColumnId[] = ['index', 'art', 'titleArtist', 'album', 'format', 'heart', 'time', 'more'];

function cell(id: ColumnId, row: TrackRow, index: number, favorite: boolean): ReactNode {
  switch (id) {
    case 'num':
      return row.trackNumber ?? '';
    case 'index':
      return index + 1;
    case 'art':
      return <Artwork artworkId={row.artworkId} size={24} radius={2} title={row.album ?? row.title} />;
    case 'title':
      return <span className="tt el">{row.title}</span>;
    case 'titleArtist':
      return (
        <span className="flex min-w-0 flex-col">
          <span className="tt el">{row.title}</span>
          <span className="el t3 text-[11.5px]">{row.artist ?? 'Unknown artist'}</span>
        </span>
      );
    case 'artist':
      return <span className="el">{row.artist ?? 'Unknown artist'}</span>;
    case 'album':
      return <span className="el">{row.album ?? ''}</span>;
    case 'year':
      return row.year ?? '';
    case 'genre':
      return <span className="el">{row.genre ?? ''}</span>;
    case 'format':
      return <span className={badgeClass(row)}>{trackBadge(row)}</span>;
    case 'rate':
      return row.sampleRate ? `${khz(row.sampleRate)}k` : '—';
    case 'bits':
      return row.lossless && row.bitDepth ? row.bitDepth : '—';
    case 'rateBits':
      return row.sampleRate ? `${khz(row.sampleRate)} kHz${row.lossless && row.bitDepth ? ` · ${row.bitDepth}` : ''}` : '—';
    case 'bitrate':
      return row.bitrateKbps ? formatKbps(row.bitrateKbps) : '—';
    case 'size':
      return formatSize(row.fileSize);
    case 'time':
      return <span style={{ color: 'var(--tx)' }}>{formatTime(row.durationMs)}</span>;
    case 'heart':
      return (
        <button
          type="button"
          className={favorite ? 'heart on' : 'heart'}
          aria-label={favorite ? 'Remove from favorites' : 'Add to favorites'}
          aria-pressed={favorite}
          onClick={(e) => {
            e.stopPropagation();
            void setFavorite([row.id], !favorite);
          }}
          onDoubleClick={(e) => e.stopPropagation()}
        >
          <Icon name="heart" size={14} filled={favorite} />
        </button>
      );
    case 'plays':
      return row.playCount || '';
    case 'added':
      return new Date(row.addedAt * 1000).toLocaleDateString('en-CA');
    case 'more':
      return null;
  }
}

interface RowProps {
  row: TrackRow | undefined;
  index: number;
  columns: ColumnId[];
  template: string;
  selected: boolean;
  selectEdge: string;
  playing: 'playing' | 'paused' | null;
  favorite: boolean;
  focused: boolean;
  onPointer: (index: number, e: MouseEvent) => void;
  onPlay: (index: number) => void;
  onMenu: (index: number, e: MouseEvent) => void;
}

const Row = memo(function Row({ row, index, columns, template, selected, selectEdge, playing, favorite, focused, onPointer, onPlay, onMenu }: RowProps) {
  if (!row) {
    return (
      <div className="tr-row tr-skeleton" style={{ gridTemplateColumns: template }} aria-hidden="true">
        {columns.map((c) => (
          <span key={c} className={c === 'title' || c === 'titleArtist' || c === 'artist' || c === 'album' ? 'sk' : ''} />
        ))}
      </div>
    );
  }
  const classes = [
    'tr-row',
    playing && 'now',
    selected && 'sel',
    selected && selectEdge,
    focused && 'focus',
    !row.available && 'gone',
  ]
    .filter(Boolean)
    .join(' ');
  return (
    <div
      className={classes}
      style={{ gridTemplateColumns: template }}
      role="row"
      aria-selected={selected}
      onMouseDown={(e) => onPointer(index, e)}
      onDoubleClick={() => onPlay(index)}
      onContextMenu={(e) => onMenu(index, e)}
    >
      {columns.map((c) => {
        const col = COLUMNS[c];
        if ((c === 'num' || c === 'index') && playing) {
          return (
            <span key={c} className="flex justify-end">
              <EqIndicator paused={playing === 'paused'} />
            </span>
          );
        }
        if (c === 'more') {
          return (
            <button
              key={c}
              type="button"
              className="ib mr"
              style={{ width: 26, height: 26 }}
              aria-label="Track options"
              onClick={(e) => {
                e.stopPropagation();
                onMenu(index, e);
              }}
            >
              <Icon name="more" size={15} strokeWidth={2.8} />
            </button>
          );
        }
        return (
          <span key={c} className={[col.mono && 'm', col.right && 'text-right', 'min-w-0'].filter(Boolean).join(' ')}>
            {cell(c, row, index, favorite)}
          </span>
        );
      })}
    </div>
  );
});

interface TrackTableProps {
  ids: number[];
  columns: ColumnId[];
  sort?: { key: TrackSort; descending: boolean };
  onSort?: (key: TrackSort) => void;
  rowHeight?: number;
  /** Plays the list starting at `index`; defaults to playing `ids`. */
  onPlay?: (index: number) => void;
  /** Extra context-menu items for the selected positions. */
  menuExtra?: (positions: number[]) => MenuItem[];
  /** Delete key on a selection (e.g. remove from playlist). */
  onDelete?: (positions: number[]) => void;
  /** Alt+↑ / Alt+↓ on a selection: move it to start at `to` (playlists). */
  onMove?: (positions: number[], to: number) => void;
  /** Summary on the right of the footer; omit to hide the footer. */
  footer?: ReactNode;
  empty?: ReactNode;
  /** Scrolls the playing track into view when the list first appears. */
  revealPlaying?: boolean;
  /** Render every row at full height inside a scrolling parent, for short
   * lists such as an album's tracks. */
  autoHeight?: boolean;
  className?: string;
}

/** A virtualized, sortable, multi-select list of library tracks. */
export function TrackTable({
  ids,
  columns,
  sort,
  onSort,
  rowHeight = 32,
  onPlay,
  menuExtra,
  onDelete,
  onMove,
  footer,
  empty,
  revealPlaying,
  autoHeight,
  className,
}: TrackTableProps) {
  const virtual = useVirtualList<HTMLDivElement>(ids.length, rowHeight);
  const list = autoHeight ? { ...virtual, start: 0, end: ids.length, offset: 0 } : virtual;
  const { rows, ensure } = useTrackRows(ids);
  const [selected, setSelected] = useState<ReadonlySet<number>>(new Set());
  const [anchor, setAnchor] = useState<number | null>(null);
  const [focus, setFocus] = useState<number | null>(null);
  const playingId = usePlayer((s) => s.track?.library?.trackId ?? null);
  const status = usePlayer((s) => s.status);
  const favorites = useLibrary((s) => s.favorites);
  const template = useMemo(() => columns.map((c) => COLUMNS[c].width).join(' '), [columns]);
  const revealed = useRef(false);

  useEffect(() => ensure(list.start, list.end), [ensure, list.start, list.end]);
  useEffect(() => {
    setSelected(new Set());
    setAnchor(null);
    setFocus(null);
  }, [ids]);
  useEffect(() => {
    if (!revealPlaying || revealed.current || playingId == null || ids.length === 0) return;
    revealed.current = true;
    const at = ids.indexOf(playingId);
    if (at > 0) list.scrollToIndex(Math.max(0, at - 3), 'start');
  }, [revealPlaying, playingId, ids, list]);

  const play = useCallback((index: number) => (onPlay ? onPlay(index) : void playTracks(ids, 'replace', index)), [ids, onPlay]);
  const positions = useCallback(
    (index?: number) => (index != null && !selected.has(index) ? [index] : [...selected].sort((a, b) => a - b)),
    [selected],
  );

  const onPointer = useCallback(
    (index: number, e: MouseEvent) => {
      if (e.button === 2 && selected.has(index)) return;
      if (e.button !== 0 && e.button !== 2) return;
      setFocus(index);
      if (e.shiftKey && anchor != null) {
        const [a, b] = anchor < index ? [anchor, index] : [index, anchor];
        const next = new Set(e.ctrlKey || e.metaKey ? selected : []);
        for (let i = a; i <= b; i++) next.add(i);
        setSelected(next);
        return;
      }
      if (e.ctrlKey || e.metaKey) {
        const next = new Set(selected);
        if (next.has(index)) next.delete(index);
        else next.add(index);
        setSelected(next);
      } else {
        setSelected(new Set([index]));
      }
      setAnchor(index);
    },
    [anchor, selected],
  );

  const showMenu = useCallback(
    (index: number, e: MouseEvent) => {
      const at = positions(index);
      if (!selected.has(index)) {
        setSelected(new Set([index]));
        setAnchor(index);
      }
      const trackIds = at.map((i) => ids[i]!).filter((id) => id != null);
      const loaded = trackIds.map((id) => rows.get(id)).filter((r): r is TrackRow => !!r);
      const { items, header } = trackMenu(trackIds, loaded, { extra: menuExtra?.(at) });
      contextMenu(() => items, { header, label: 'Track actions' })(e);
    },
    [ids, menuExtra, positions, rows, selected],
  );

  const onKeyDown = (e: KeyboardEvent) => {
    const current = focus ?? -1;
    const move = (to: number) => {
      const index = Math.max(0, Math.min(ids.length - 1, to));
      setFocus(index);
      list.scrollToIndex(index);
      if (e.shiftKey && anchor != null) {
        const [a, b] = anchor < index ? [anchor, index] : [index, anchor];
        const next = new Set<number>();
        for (let i = a; i <= b; i++) next.add(i);
        setSelected(next);
      } else {
        setSelected(new Set([index]));
        setAnchor(index);
      }
    };
    const chosen = positions(focus ?? undefined);
    const chosenIds = chosen.map((i) => ids[i]!).filter((id) => id != null);
    const page = Math.max(1, Math.floor((list.ref.current?.clientHeight ?? 400) / rowHeight) - 1);
    let handled = true;
    switch (e.key) {
      case 'ArrowDown':
      case 'ArrowUp': {
        const down = e.key === 'ArrowDown';
        if (e.altKey && onMove && chosen.length) {
          const first = chosen[0]!;
          const to = down ? Math.min(ids.length - chosen.length, first + 1) : Math.max(0, first - 1);
          if (to !== first) {
            onMove(chosen, to);
            setSelected(new Set(chosen.map((_, i) => to + i)));
            setFocus(to);
            setAnchor(to);
          }
          break;
        }
        move(current + (down ? 1 : -1));
        break;
      }
      case 'PageDown':
        move(current + page);
        break;
      case 'PageUp':
        move(current - page);
        break;
      case 'Home':
        move(0);
        break;
      case 'End':
        move(ids.length - 1);
        break;
      case 'Enter':
        if (e.shiftKey) void playTracks(chosenIds, 'next');
        else if (e.ctrlKey) void playTracks(chosenIds, 'append');
        else if (chosen.length > 1) void playTracks(chosenIds);
        else if (focus != null) play(focus);
        break;
      case 'q':
      case 'Q':
        if (e.ctrlKey || e.altKey) handled = false;
        else void playTracks(chosenIds, 'append');
        break;
      case 'l':
      case 'L':
        if (e.ctrlKey || e.altKey) handled = false;
        else void setFavorite(chosenIds, !chosenIds.every((id) => favorites.has(id)));
        break;
      case 'a':
        if (e.ctrlKey) setSelected(new Set(ids.map((_, i) => i)));
        else if (e.altKey && focus != null) {
          const row = rows.get(ids[focus]!);
          if (row?.albumId != null) navigate('album', row.albumId);
        } else handled = false;
        break;
      case 'r':
        if (e.altKey && focus != null) {
          const row = rows.get(ids[focus]!);
          if (row?.artistId != null) navigate('artists', row.artistId);
        } else handled = false;
        break;
      case 'i':
        if (e.ctrlKey && focus != null) {
          const row = rows.get(ids[focus]!);
          if (row) void showTrackInfo(row);
        } else handled = false;
        break;
      case 'Delete':
        if (onDelete && chosen.length) onDelete(chosen);
        else handled = false;
        break;
      case 'ContextMenu': {
        if (focus == null) break;
        const el = list.ref.current?.querySelector<HTMLElement>('.tr-row.focus');
        if (el) {
          const r = el.getBoundingClientRect();
          showMenu(focus, { clientX: r.left + 40, clientY: r.bottom, preventDefault() {}, stopPropagation() {} } as MouseEvent);
        }
        break;
      }
      case 'Escape':
        if (selected.size) setSelected(new Set());
        else handled = false;
        break;
      default:
        handled = false;
    }
    if (handled) {
      e.preventDefault();
      e.stopPropagation();
    }
  };

  const selection = useMemo(() => {
    if (selected.size < 2) return null;
    const picked = [...selected].map((i) => rows.get(ids[i]!)).filter((r): r is TrackRow => !!r);
    const formats = new Set(picked.map(trackBadge));
    return {
      count: selected.size,
      duration: picked.reduce((t, r) => t + r.durationMs, 0),
      size: picked.reduce((t, r) => t + r.fileSize, 0),
      format: formats.size === 1 ? [...formats][0] : null,
    };
  }, [selected, rows, ids]);

  const visible = [];
  for (let i = list.start; i < list.end; i++) {
    const id = ids[i]!;
    const row = rows.get(id);
    const isSelected = selected.has(i);
    const edge = isSelected ? `${selected.has(i - 1) ? '' : 'first'} ${selected.has(i + 1) ? '' : 'last'}`.trim() || 'mid' : '';
    visible.push(
      <Row
        key={`${i}:${id}`}
        row={row}
        index={i}
        columns={columns}
        template={template}
        selected={isSelected}
        selectEdge={edge}
        playing={row && row.id === playingId ? (status === 'playing' || status === 'loading' ? 'playing' : 'paused') : null}
        favorite={favorites.has(id)}
        focused={focus === i}
        onPointer={onPointer}
        onPlay={play}
        onMenu={showMenu}
      />,
    );
  }

  return (
    <div className={['tt-wrap', autoHeight && 'auto', className].filter(Boolean).join(' ')}>
      <div className="tr-row tr-head" style={{ gridTemplateColumns: template }} role="row">
        {columns.map((c) => {
          const col = COLUMNS[c];
          const active = sort && col.sort && sort.key === col.sort;
          const label = c === 'num' || c === 'index' ? '#' : col.label;
          if (!onSort || !col.sort) {
            return (
              <span key={c} className={col.right ? 'text-right' : undefined} role="columnheader">
                {label}
              </span>
            );
          }
          return (
            <button
              key={c}
              type="button"
              role="columnheader"
              aria-sort={active ? (sort!.descending ? 'descending' : 'ascending') : 'none'}
              className={['th', active && 'on', col.right && 'justify-end'].filter(Boolean).join(' ')}
              onClick={() => onSort(col.sort!)}
            >
              {label}
              {active && <Icon name={sort!.descending ? 'chevD' : 'chevU'} size={11} strokeWidth={2.4} />}
            </button>
          );
        })}
      </div>
      {ids.length === 0 && empty ? (
        <div className="tt-empty">{empty}</div>
      ) : (
        <div
          ref={list.ref}
          className="tt-scroll"
          tabIndex={0}
          role="grid"
          aria-rowcount={ids.length}
          aria-multiselectable="true"
          onKeyDown={onKeyDown}
          onFocus={() => focus == null && ids.length > 0 && setFocus(list.start)}
        >
          <div style={{ height: list.totalHeight, position: 'relative' }}>
            <div className="tt-rows" style={{ transform: `translateY(${list.offset}px)`, ['--row-h' as string]: `${rowHeight}px` }}>
              {visible}
            </div>
          </div>
        </div>
      )}
      {footer !== undefined && (
        <div className="tt-foot mono">
          <span className="el">
            {selection ? (
              <>
                <span style={{ color: 'var(--ac)' }}>{selection.count} selected</span> · {formatLength(selection.duration)} ·{' '}
                {formatSize(selection.size)}
                {selection.format && ` · ${selection.format}`}
              </>
            ) : (
              footer
            )}
          </span>
          <span className="t3 flex-none">{ids.length.toLocaleString('en-US')} rows</span>
        </div>
      )}
    </div>
  );
}
