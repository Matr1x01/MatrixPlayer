# Fermata

An audio-focused music player for Linux, in the spirit of Poweramp: native
playback on GStreamer and PipeWire, a Tauri 2 shell, and a React UI built from
the design in `Fermata — Linux Music Player.html`.

**Status: milestone 1 (foundation and native playback).** Files and folders
play through the real GStreamer pipeline with gapless transitions; the library
(indexing, albums, search) is the next milestone.

## Requirements

- Rust 1.92+ and Node 20+
- GStreamer 1.22+ with `gst-plugins-base` and `gst-plugins-good` (FLAC, WAV,
  Vorbis, Opus); `gst-libav` or `gst-plugins-bad` for AAC/ALAC; the
  `pipewire` GStreamer plugin for native PipeWire output (PulseAudio and ALSA
  are fallbacks)
- WebKitGTK 4.1 and the usual [Tauri Linux prerequisites](https://tauri.app/start/prerequisites/)

## Commands

```sh
npm install
npm run tauri dev                   # run the app
npm run tauri dev -- -- ~/Music     # …and queue a folder or files from the command line
npm run check                       # type-check, frontend tests, Rust tests
cargo test --manifest-path src-tauri/Cargo.toml -- --ignored   # + a muted run through the real sound server
npx tauri build                     # release build and packages
```

`FERMATA_AUDIO_SINK=pipewire|pulse|alsa|auto` forces an output backend.
`RUST_LOG=fermata_lib=debug` shows pipeline decisions (gapless queueing, errors).

## Architecture

```text
React UI (src/)               renders state, sends commands; never holds playback state
   │  commands ▲ events
Tauri glue (src-tauri/src/commands, events.rs)
   │
Player service (player/)      single task owning the queue and the authoritative state
   │
Audio engine (audio/, dsp/)   GStreamer playbin3 ─ 64-bit float DSP chain ─ output sink
   │
PipeWire  (PulseAudio / ALSA fallbacks)
```

Rust (`src-tauri/src`):

| Module | Responsibility |
| --- | --- |
| `audio/` | `AudioEngine`: one `playbin3` for the app's lifetime. Gapless via `about-to-finish`; bus messages are tagged with a pipeline *epoch* at post time so events from a replaced stream are dropped. Reports the negotiated decoded and output formats. |
| `dsp/` | The processing chain installed as playbin's `audio-filter`, in F64. Volume lives here today; ReplayGain, EQ and the rest slot in before it. |
| `player/` | `Queue` (play order, shuffle, repeat) and the player service actor. Commands and engine events are handled on one task; broken files are reported and skipped. |
| `metadata/` | Tags and stream properties via `lofty`; fields Fermata infers are listed in `derivedFields`. |
| `artwork/` | Content-addressed artwork cache served to the webview through the `art://` scheme (no filesystem access from the frontend). |
| `database/` | SQLite via SQLx with embedded migrations. Holds settings today; library tables arrive with the scanner. |
| `preferences.rs` | Validated UI preferences (theme, accent, density, scale…). |
| `filesystem/` | Audio file detection and path validation. |

Frontend (`src/`): `app/` (shell, routing), `components/` (design-system
primitives, icons, title bar, sidebar), `features/` (player, now-playing,
queue, library, settings, sound), `stores/` (zustand), `services/` (typed IPC).

### IPC contract

Commands: `get_player_snapshot`, `player_{play,pause,toggle,stop,next,previous,seek,set_volume,set_muted,set_repeat,set_shuffle}`,
`queue_{play_entry,remove_entry,clear}`, `open_audio_files`, `open_audio_folder`,
`get_app_info`, `get_ui_preferences`, `update_ui_preferences`.

Events: `playback-state-changed`, `track-changed`, `position-changed` (every
200 ms while playing), `volume-changed`, `queue-changed` (contents), `queue-state-changed` (current
entry and modes), `signal-path-changed`, `audio-error`, `tracks-queued`, `player-resync`.

File paths never travel from the frontend to Rust: files arrive through the
native dialog (opened by Rust), drag and drop onto the window, or the command
line.

## Design

The design file is the visual source of truth. `npm run extract-design`
unpacks its 15 boards into `design/extracted/` (gitignored) and its fonts into
`src/assets/fonts/`. Class names in `src/styles/components.css` follow the
design's CSS so markup can be compared side by side.

`npm run dev`, then open `http://localhost:5173/dev/harness.html?view=nowPlaying`
to render any view in a normal browser against mocked IPC and the design's
sample data (`view`, `theme`, `accent` and `empty` query parameters).

## Roadmap

1. **Foundation and playback** — done: design system and shell, native
   GStreamer playback, play/pause/seek/next/previous, volume, gapless,
   shuffle/repeat, queue, artwork, format details, signal path, preferences.
2. **Library** — folder scanning, metadata in SQLite, artists/albums/genres/folders, search.
3. **Queue and playlists** — reordering, playlists, favourites, history.
4. **Advanced player** — ReplayGain, lyrics (LRC), visualizers, waveform seek.
5. **DSP** — preamp, graphic and parametric EQ, presets.
6. **Linux integration** — MPRIS, media keys, notifications, file associations, packaging.
