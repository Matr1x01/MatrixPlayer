import { beforeEach, describe, expect, it } from 'vitest';

import { useNavigation } from './navigation';

const current = () => {
  const { history, index } = useNavigation.getState();
  return history[index];
};
const views = () => useNavigation.getState().history.map((r) => (r.id == null ? r.view : `${r.view}:${r.id}`));

describe('navigation history', () => {
  beforeEach(() => useNavigation.setState({ history: [{ view: 'home' }], index: 0 }));

  it('moves back and forward through visited views', () => {
    const nav = useNavigation.getState();
    nav.navigate('queue');
    nav.navigate('settings');
    nav.back();
    expect(current()?.view).toBe('queue');
    nav.back();
    nav.back();
    expect(current()?.view).toBe('home');
    nav.forward();
    expect(current()?.view).toBe('queue');
  });

  it('drops the forward history when navigating somewhere new', () => {
    const nav = useNavigation.getState();
    nav.navigate('queue');
    nav.navigate('settings');
    nav.back();
    nav.navigate('dsp');
    expect(views()).toEqual(['home', 'queue', 'dsp']);
    nav.forward();
    expect(current()?.view).toBe('dsp');
  });

  it('ignores navigating to the current route', () => {
    useNavigation.getState().navigate('home');
    expect(views()).toEqual(['home']);
  });

  it('keeps detail routes apart by id', () => {
    const nav = useNavigation.getState();
    nav.navigate('album', 3);
    nav.navigate('album', 3);
    nav.navigate('album', 9);
    expect(views()).toEqual(['home', 'album:3', 'album:9']);
    nav.replace({ view: 'album', id: 10 });
    expect(views()).toEqual(['home', 'album:3', 'album:10']);
  });
});
