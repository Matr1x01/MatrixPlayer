import { Icon } from '@/components/icons/Icon';
import { IconButton } from '@/components/ui/IconButton';
import { useNavigation, useView, type ViewId } from '@/stores/navigation';
import { usePlayer } from '@/stores/player';
import { updatePreferences } from '@/stores/preferences';

import { NAV_GROUPS } from './navItems';

/** Counts shown beside nav items. Only views with real data get one. */
function useCounts(): Partial<Record<ViewId, string>> {
  const queueLength = usePlayer((s) => s.queue.entries.length);
  return queueLength > 0 ? { queue: queueLength.toLocaleString('en-US') } : {};
}

/** The full 220 px sidebar (Library — Albums board). */
export function Sidebar() {
  const view = useView();
  const navigate = useNavigation((s) => s.navigate);
  const counts = useCounts();

  return (
    <nav className="sb" aria-label="Library navigation">
      {NAV_GROUPS.map((group) => (
        <div key={group.label} className="sb-group">
          <div className="cap sb-cap">{group.label}</div>
          {group.items.map((item) => (
            <button
              key={item.view}
              type="button"
              className={view === item.view ? 'nv act' : 'nv'}
              aria-current={view === item.view ? 'page' : undefined}
              onClick={() => navigate(item.view)}
            >
              <Icon name={item.icon} />
              <span className="el">{item.label}</span>
              {counts[item.view] && <span className="ct mono">{counts[item.view]}</span>}
            </button>
          ))}
        </div>
      ))}

      <div className="sb-foot">
        <button
          type="button"
          className={view === 'settings' ? 'nv act' : 'nv'}
          aria-current={view === 'settings' ? 'page' : undefined}
          onClick={() => navigate('settings')}
        >
          <Icon name="gear" />
          <span>Settings</span>
        </button>
        <div className="sb-status">
          <span className="dot" style={{ background: 'var(--tx3)' }} />
          <span className="mono t3 el flex-1 text-[10px]">No library indexed yet</span>
          <IconButton
            icon="side"
            label="Collapse sidebar to icon rail"
            size={26}
            iconSize={16}
            onClick={() => void updatePreferences({ sidebarExpanded: false })}
          />
        </div>
      </div>
    </nav>
  );
}
