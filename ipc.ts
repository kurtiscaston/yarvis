// The only file that knows the UI talks to a Rust core over Tauri.
// Opened in a plain browser (npm run dev), it swaps in sample data instead.

import type { Summary } from './stats';

export interface Hit {
  id: string;
  title: string;
  kind: string;
  /** Character positions in `title` that matched the query. */
  matched: number[];
}

export interface Theme {
  id: string;
  name: string;
  appearance: 'dark' | 'light';
  tokens: Record<string, string>;
}

export interface Settings {
  hotkey: string;
  theme: string;
  hideOnBlur: boolean;
  showTimings: boolean;
}

export interface Bootstrap {
  theme: Theme;
  settings: Settings;
  problems: string[];
}

export interface SearchResponse {
  hits: Hit[];
  searchUs: number;
  indexed: number;
}

export interface HotkeyTimings {
  shown: Summary;
  firstFrame: Summary;
}

export interface Events {
  'launcher-shown': number | null;
  'launcher-reset': null;
  'theme-changed': Theme;
  'settings-changed': Settings;
  notice: string;
}

export interface Backend {
  bootstrap(): Promise<Bootstrap>;
  search(query: string): Promise<SearchResponse>;
  activate(id: string): Promise<void>;
  hide(): Promise<void>;
  firstFrame(seq: number): Promise<HotkeyTimings>;
  on<K extends keyof Events>(event: K, handler: (payload: Events[K]) => void): Promise<void>;
}

async function tauriBackend(): Promise<Backend> {
  const { invoke } = await import('@tauri-apps/api/core');
  const { listen } = await import('@tauri-apps/api/event');
  return {
    bootstrap: () => invoke('bootstrap'),
    search: (query) => invoke('search', { query }),
    activate: (id) => invoke('activate', { id }),
    hide: () => invoke('hide'),
    firstFrame: (seq) => invoke('first_frame', { seq }),
    on: async (event, handler) => {
      await listen(event, (message) => handler(message.payload as never));
    },
  };
}

export const inTauri = '__TAURI_INTERNALS__' in window;

export async function connect(): Promise<Backend> {
  if (inTauri) return tauriBackend();
  document.documentElement.classList.add('preview');
  return (await import('./preview')).previewBackend();
}
