import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import './styles/index.css';
import './app/app.css';
import './components/shell/shell.css';
import './features/player/player.css';
import './features/queue/queue.css';
import './features/now-playing/now-playing.css';
import './features/settings/settings.css';
import './features/sound/sound.css';
import './features/library/library.css';
import './styles/motion.css';

import { App } from './app/App';

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
