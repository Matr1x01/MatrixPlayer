# MatrixPlayer

An audio-focused music player for Linux, in the spirit of Poweramp: native
playback on GStreamer and PipeWire with a 64-bit float processing chain, a
Tauri 2 shell, and a React UI built from the design in
`Fermata — Linux Music Player.html`.

**Status: all six roadmap milestones are in.** Library, queue and playlists,
ReplayGain, parametric/graphic EQ and DSP, visualizers, synced lyrics,
waveform seek, output device selection, MPRIS, notifications and desktop
integration all run on the real pipeline. See [Not yet](#not-yet) for what is
deliberately left as an extension point.

## Requirements

- Rust 1.92+ and Node 20+
- GStreamer 1.22+ with `gst-plugins-base` and `gst-plugins-good` (FLAC, WAV,
  Vorbis, Opus); `gst-libav` or `gst-plugins-bad` for AAC/ALAC; the
  `pipewire` GStreamer plugin for native PipeWire output (PulseAudio and ALSA
  are fallbacks)
- WebKitGTK 4.1 and the usual [Tauri Linux prerequisites](https://tauri.app/start/prerequisites/)
- A D-Bus session bus for MPRIS and notifications (optional; both are skipped without one)

## Commands

```sh
npm install
npm run tauri dev                   # run the app
npm run tauri dev -- -- ~/Music     # …and play a folder or files from the command line
npm run check                       # type-check, frontend tests, Rust tests
cargo test --manifest-path src-tauri/Cargo.toml -- --ignored   # + runs against a real sound server
npx tauri build                     # release build and packages (.deb, .rpm, AppImage)
```

`MATRIXPLAYER_AUDIO_SINK=pipewire|pulse|alsa|auto|null` forces an output backend
(`null` plays in real time into nothing, for headless runs).
`RUST_LOG=matrixplayer_lib=debug` shows pipeline decisions (gapless queueing, errors).

A running instance also takes these from the command line or the desktop
entry's actions: `matrixplayer --play-pause | --play | --pause | --stop | --next | --previous`,
or files and `file://` URIs to play.

## Architecture

```text
React UI (src/)               renders state, sends commands; never holds playback state
   │  commands ▲ events
Tauri glue (src-tauri/src/commands, events.rs)          MPRIS / notifications (desktop/)
   │                                                        │ same commands, same events
Services: player/ · library/ · dsp/service.rs ◀─────────────┘
   │
Audio engine (audio/, dsp/)   GStreamer playbin3 ─ F64 DSP chain ─ volume ─ output sink
   │
PipeWire  (PulseAudio / ALSA fallbacks)
```

Rust (`src-tauri/src`):

| Module | Responsibility |
| --- | --- |
| `audio/` | `AudioEngine`: one `playbin3` for the app's lifetime. Gapless via `about-to-finish`; bus messages are tagged with a pipeline *epoch* at post time so events from a replaced stream are dropped. Output backends and devices (listed through the backend's GStreamer device provider), negotiated formats, and waveform overviews decoded off the playback path and cached. |
| `dsp/` | The `audio-filter` chain, in 64-bit float: ReplayGain from in-stream tags (exact at gapless joins) with peak-based clipping prevention, preamp and auto preamp, parametric and graphic EQ (RBJ biquads), bass/treble shelves, balance, stereo width, crossfeed, soft fades, a peak limiter, and an analysis tap (FFT spectrum, scope, peak/RMS) aligned with what is audible. `service.rs` persists settings and presets. |
| `player/` | `Queue` (play order, shuffle, repeat, reordering) and the player service actor. Commands, engine events and timers are handled on one task: soft pause/resume, gapless toggle, output switching at the same position, session save/restore, broken files reported and skipped. |
| `library/` | Incremental scanner (stat-only walk, size/mtime diff, rename detection, parallel tag reads), file watcher, SQLite queries for albums/artists/genres/folders, FTS5 search with accent folding, favorites, playlists and listening history. |
| `metadata/` | Tags and stream properties via `lofty`; lyrics from a sidecar `.lrc` or embedded tags (LRC with offsets and word timings). |
| `artwork/` | Content-addressed artwork cache with thumbnails, served through the `art://` scheme (no filesystem access from the frontend). |
| `desktop/` | MPRIS 2 on the session bus (desktop controls, lock screens, media keys), track notifications, command-line actions. |
| `database/` | SQLite via SQLx with embedded migrations. |
| `preferences.rs` | Validated UI preferences (theme, accent, density, scale, notifications…). |

Frontend (`src/`): `app/` (shell, routing), `components/` (design-system
primitives, icons, title bar, sidebar), `features/` (player, now-playing,
queue, library, search, settings, sound), `stores/` (zustand mirrors of core
state), `services/` (typed IPC, the shared analysis stream), `styles/`
(tokens, components, the motion layer).

### IPC contract

Commands (`src/services/ipc.ts` wraps every one):

- Player: `get_player_snapshot`, `player_{play,pause,toggle,stop,next,previous,seek,set_volume,set_muted,set_repeat,set_shuffle,set_gapless,set_output_device}`, `player_lyrics`, `player_waveform`, `audio_output_devices`
- Queue: `queue_{play_entry,remove_entry,clear,clear_others,move,saved_session,restore_session}`, `open_audio_files`, `open_audio_folder`
- Library: `library_{stats,scan_status,roots,add_root,add_default_root,default_root,remove_root,rescan}`, `library_{albums,album,artists,artist,genres,genre,folder_children,folder,track_list,tracks,search,play}`, `library_{set_favorite,favorite_ids,history,clear_history,reveal_folder,track_folder}`
- Playlists: `playlists_list`, `playlist_{get,create,rename,set_pinned,delete,add,remove,move}`
- DSP: `dsp_{state,update,restore_defaults,save_preset,delete_preset,export_presets,import_presets,set_analysis}`
- System: `get_app_info`, `get_ui_preferences`, `update_ui_preferences`

Events: `playback-state-changed`, `track-changed`, `position-changed` (every
200 ms while playing), `volume-changed`, `queue-changed` (contents),
`queue-state-changed` (current entry and modes), `signal-path-changed`,
`audio-error`, `tracks-queued`, `player-resync`, `library-scan`,
`library-changed`, `dsp-status` (ReplayGain source, limiter), `audio-analysis`
(~30 frames/s, only while `dsp_set_analysis(true)`).

File paths never travel from the frontend to Rust: files arrive through the
native dialogs (opened by Rust), drag and drop onto the window, the command
line, a second launch, or MPRIS `OpenUri`.

## Design

The design file is the visual source of truth. `npm run extract-design`
unpacks its 15 boards into `design/extracted/` (gitignored) and its fonts into
`src/assets/fonts/`. Class names in `src/styles/components.css` follow the
design's CSS so markup can be compared side by side.

`npm run dev`, then open `http://localhost:5173/dev/harness.html?view=equalizer`
to render any view in a normal browser against mocked IPC and the design's
sample data (`view`, `id`, `tab`, `theme`, `accent`, `empty`, `noroots`,
`scanning`, `search` and `menu` query parameters). The harness feeds a
synthetic analysis stream so visualizers move.

Motion follows the design's timing tokens (`styles/motion.css`) and is
switched off by the *Reduce motion* preference and by the system's
`prefers-reduced-motion`.

## Packaging

`npx tauri build` produces `.deb`, `.rpm` and AppImage packages. The desktop
entry (`src-tauri/linux/matrixplayer.desktop`) registers the audio MIME types
and launcher actions (Play/Pause, Next, Previous); AppStream metadata ships in
`src-tauri/linux/io.github.matr1x01.MatrixPlayer.metainfo.xml`. A Flatpak
manifest is not written yet.

## Roadmap

1. **Foundation and playback** — done.
2. **Library** — done: incremental scanning and watching, SQLite metadata, albums/artists/genres/folders, instant search.
3. **Queue and playlists** — done: reordering, playlists, favorites, history, session restore.
4. **Advanced player** — done: ReplayGain, synced lyrics (LRC), visualizers, waveform seek.
5. **DSP** — done: preamp and auto preamp, graphic (10/31) and parametric EQ, tone and stereo controls, crossfeed, presets with import/export, A/B compare.
6. **Linux integration** — done: MPRIS, media keys (via MPRIS), notifications, file associations, single instance, `.deb`/`.rpm`/AppImage packaging.

### Not yet

These have a place in the UI marked as not available, rather than a fake:

- Crossfade (tracks join gaplessly instead)
- Exclusive device access and forcing an output sample rate or bit depth
- Per-device EQ presets
- A Flatpak manifest
