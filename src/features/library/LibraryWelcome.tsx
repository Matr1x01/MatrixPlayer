import { useEffect, useState } from 'react';

import { Icon } from '@/components/icons/Icon';
import { libraryApi } from '@/services/ipc';
import { run, showToast } from '@/stores/toasts';
import type { DefaultRoot } from '@/types/library';

import { openFolder } from './openActions';

const FORMATS = 'FLAC · ALAC · WAV · AIFF · DSF/DFF · MP3 · AAC · Opus · Vorbis · WavPack · APE';

export async function addLibraryFolder() {
  const id = await run(libraryApi.addRoot());
  if (id != null) showToast({ tone: 'info', message: 'Folder added — indexing your music' });
}

/** First run (Empty & loading states · 01): point MatrixPlayer at your music. */
export function LibraryWelcome() {
  const [music, setMusic] = useState<DefaultRoot | null>(null);
  useEffect(() => {
    void libraryApi
      .defaultRoot()
      .then(setMusic)
      .catch(() => setMusic(null));
  }, []);

  return (
    <div className="welcome">
      <svg viewBox="0 0 120 80" width={132} height={88} fill="none" aria-hidden="true" className="welcome-mark">
        <path d="M14 70a46 46 0 0 1 92 0" stroke="var(--ln2)" strokeWidth="1.5" />
        <path d="M28 70a32 32 0 0 1 64 0" stroke="var(--ln2)" strokeWidth="1.5" />
        <path d="M42 70a18 18 0 0 1 36 0" stroke="var(--ac)" strokeWidth="3" strokeLinecap="round" />
        <circle cx="60" cy="70" r="5" fill="var(--ac)" />
      </svg>
      <h2 className="text-[22px] font-bold tracking-[-0.01em]">Point MatrixPlayer at your music</h2>
      <p className="t2 max-w-[420px] [text-wrap:pretty]">
        Add the folders where you keep your files. MatrixPlayer reads tags, artwork and audio details — your files stay
        exactly where they are.
      </p>
      <div className="mt-1 flex flex-wrap justify-center gap-2">
        <button type="button" className="btn pri lg" onClick={() => void addLibraryFolder()}>
          <Icon name="plus" size={16} />
          Add music folder…
        </button>
        {music && music.files > 0 && (
          <button
            type="button"
            className="btn lg"
            style={{ fontWeight: 500 }}
            onClick={() => void run(libraryApi.addDefaultRoot())}
          >
            Use <span className="mono text-[11.5px]">{music.display}</span>
            <span className="mono t3 text-[10.5px]">
              {music.files >= 50_000 ? '50,000+' : music.files.toLocaleString('en-US')} files found
            </span>
          </button>
        )}
      </div>
      <button type="button" className="btn gh" style={{ fontWeight: 500 }} onClick={() => void openFolder()}>
        Or play a folder without adding it
        <span className="kbd">Ctrl Shift O</span>
      </button>
      <span className="mono t3 text-[10.5px]">{FORMATS}</span>
    </div>
  );
}
