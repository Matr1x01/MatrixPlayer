import { Icon } from '@/components/icons/Icon';
import { PARENT_VIEW, useNavigation, useView } from '@/stores/navigation';
import { updatePreferences } from '@/stores/preferences';

import { NAV_GROUPS } from './navItems';

/** The 60 px icon rail used by Now Playing and collapsed layouts. */
export function SidebarRail({ canExpand }: { canExpand: boolean }) {
  const current = useView();
  const view = PARENT_VIEW[current] ?? current;
  const navigate = useNavigation((s) => s.navigate);

  return (
    <nav className="sb rail" aria-label="Library navigation">
      {NAV_GROUPS.map((group) => (
        <div key={group.label} className="rail-group">
          {group.items.map((item) => (
            <button
              key={item.view}
              type="button"
              className={view === item.view ? 'nv act' : 'nv'}
              aria-label={item.label}
              aria-current={view === item.view ? 'page' : undefined}
              data-tip={item.label}
              onClick={() => navigate(item.view)}
            >
              <Icon name={item.icon} />
            </button>
          ))}
        </div>
      ))}
      <div className="rail-group mt-auto">
        <button
          type="button"
          className={view === 'settings' ? 'nv act' : 'nv'}
          aria-label="Settings"
          data-tip="Settings"
          onClick={() => navigate('settings')}
        >
          <Icon name="gear" />
        </button>
        {canExpand && (
          <button
            type="button"
            className="nv"
            aria-label="Expand sidebar"
            data-tip="Expand sidebar"
            onClick={() => void updatePreferences({ sidebarExpanded: true })}
          >
            <Icon name="side" size={16} />
          </button>
        )}
      </div>
    </nav>
  );
}
