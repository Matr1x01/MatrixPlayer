import { beforeEach, describe, expect, it } from 'vitest';

import { useNavigation } from './navigation';

const current = () => {
  const { history, index } = useNavigation.getState();
  return history[index];
};

describe('navigation history', () => {
  beforeEach(() => useNavigation.setState({ history: ['home'], index: 0 }));

  it('moves back and forward through visited views', () => {
    const nav = useNavigation.getState();
    nav.navigate('queue');
    nav.navigate('settings');
    nav.back();
    expect(current()).toBe('queue');
    nav.back();
    nav.back();
    expect(current()).toBe('home');
    nav.forward();
    expect(current()).toBe('queue');
  });

  it('drops the forward history when navigating somewhere new', () => {
    const nav = useNavigation.getState();
    nav.navigate('queue');
    nav.navigate('settings');
    nav.back();
    nav.navigate('dsp');
    expect(useNavigation.getState().history).toEqual(['home', 'queue', 'dsp']);
    nav.forward();
    expect(current()).toBe('dsp');
  });

  it('ignores navigating to the current view', () => {
    useNavigation.getState().navigate('home');
    expect(useNavigation.getState().history).toEqual(['home']);
  });
});
