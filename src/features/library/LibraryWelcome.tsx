import { Icon } from '@/components/icons/Icon';

import { openFiles, openFolder } from './openActions';

const FORMATS = 'FLAC · ALAC · WAV · AIFF · MP3 · AAC · Opus · Vorbis · WavPack · APE';

/**
 * First run (Empty states · 01). Library indexing is the next milestone, so
 * the actions here play folders and files directly through the queue.
 */
export function LibraryWelcome() {
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-4 px-20 py-10 text-center">
      <svg viewBox="0 0 120 80" width={132} height={88} fill="none" aria-hidden="true">
        <path d="M14 70a46 46 0 0 1 92 0" stroke="var(--ln2)" strokeWidth="1.5" />
        <path d="M28 70a32 32 0 0 1 64 0" stroke="var(--ln2)" strokeWidth="1.5" />
        <path d="M42 70a18 18 0 0 1 36 0" stroke="var(--ac)" strokeWidth="3" strokeLinecap="round" />
        <circle cx="60" cy="70" r="5" fill="var(--ac)" />
      </svg>
      <h2 className="text-[22px] font-bold tracking-[-0.01em]">Point Fermata at your music</h2>
      <p className="t2 max-w-[440px] [text-wrap:pretty]">
        Open a folder or a few files to start listening. Fermata reads tags, artwork and audio details — your files stay
        exactly where they are. Library indexing, with albums, artists and search, is coming next.
      </p>
      <div className="mt-1 flex gap-2">
        <button type="button" className="btn pri lg" onClick={() => void openFolder()}>
          <Icon name="folder" size={16} />
          Open music folder…
        </button>
        <button type="button" className="btn lg" style={{ fontWeight: 500 }} onClick={() => void openFiles()}>
          <Icon name="plus" size={16} />
          Open files…
          <span className="kbd">Ctrl O</span>
        </button>
      </div>
      <span className="mono t3 text-[10.5px]">{FORMATS}</span>
    </div>
  );
}
