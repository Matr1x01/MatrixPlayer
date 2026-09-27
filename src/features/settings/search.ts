import { create } from 'zustand';

/** The query in the title bar's settings search, read by the settings view. */
export const useSettingsSearch = create<{ query: string }>()(() => ({ query: '' }));

export function setSettingsQuery(query: string) {
  useSettingsSearch.setState({ query });
}
