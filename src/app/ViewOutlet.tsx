import { HomeView, LibraryPlaceholderView } from '@/features/library/LibraryViews';
import { NowPlayingView } from '@/features/now-playing/NowPlayingView';
import { QueueView } from '@/features/queue/QueueView';
import { SettingsView } from '@/features/settings/SettingsView';
import { DspView, EqualizerView } from '@/features/sound/SoundViews';
import type { ViewId } from '@/stores/navigation';

export function ViewOutlet({ view }: { view: ViewId }) {
  switch (view) {
    case 'home':
      return <HomeView />;
    case 'queue':
      return <QueueView />;
    case 'nowPlaying':
      return <NowPlayingView />;
    case 'equalizer':
      return <EqualizerView />;
    case 'dsp':
      return <DspView />;
    case 'settings':
      return <SettingsView />;
    default:
      return <LibraryPlaceholderView view={view} />;
  }
}
