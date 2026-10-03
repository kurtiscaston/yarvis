// Sample data for looking at the UI in a browser, without the Rust core.
// Lazy-loaded, so none of it runs inside the real app.

import type { Backend, Events, Hit, Theme } from './ipc';
import { NO_SAMPLES } from './stats';

const themeFiles = import.meta.glob<Omit<Theme, 'id'>>('../../themes/*.json', {
  eager: true,
  import: 'default',
});
const themes: Theme[] = Object.entries(themeFiles).map(([path, theme]) => ({
  ...theme,
  id: path.split('/').pop()!.replace('.json', ''),
}));

const apps = [
  'Calculator', 'Calendar', 'Camera', 'Clock', 'Command Prompt', 'Control Panel',
  'File Explorer', 'Firefox', 'Google Chrome', 'Maps', 'Notepad', 'Paint', 'Photos',
  'PowerShell', 'Settings', 'Snipping Tool', 'Spotify', 'Task Manager', 'Terminal',
  'Visual Studio Code',
];
const commands = ['Measure idle CPU for 30 seconds', 'Open themes folder', 'Quit Launcher', 'Reload themes'];

const items: Hit[] = [
  ...apps.map((title) => ({ id: `app:${title}`, title, kind: 'Application', matched: [] })),
  ...commands.map((title) => ({ id: `cmd:${title}`, title, kind: 'Command', matched: [] })),
  ...themes.map((theme) => ({ id: `theme:${theme.id}`, title: `Use ${theme.name} theme`, kind: 'Command', matched: [] })),
].sort((a, b) => a.title.localeCompare(b.title));

/** In-order character match; a stand-in for the real fuzzy matcher. */
function match(title: string, query: string): number[] | null {
  const chars = Array.from(title.toLowerCase());
  const matched: number[] = [];
  let from = 0;
  for (const wanted of Array.from(query.toLowerCase().replace(/\s+/g, ''))) {
    const at = chars.indexOf(wanted, from);
    if (at < 0) return null;
    matched.push(at);
    from = at + 1;
  }
  return matched;
}

export function previewBackend(): Backend {
  const handlers: { [K in keyof Events]?: (payload: Events[K]) => void } = {};
  const wanted = new URLSearchParams(location.search).get('theme');
  const initial = themes.find((theme) => theme.id === wanted) ?? themes.find((theme) => theme.id === 'dusk')!;

  return {
    bootstrap: async () => ({
      theme: initial,
      settings: { hotkey: 'Ctrl+Shift+Space', theme: initial.id, hideOnBlur: true, showTimings: true },
      problems: [],
    }),
    search: async (query) => {
      const started = performance.now();
      const hits = items.flatMap((item) => {
        const matched = match(item.title, query.trim());
        return matched ? [{ ...item, matched }] : [];
      });
      return { hits, searchUs: (performance.now() - started) * 1000, indexed: items.length };
    },
    activate: async (id) => {
      const theme = themes.find((theme) => `theme:${theme.id}` === id);
      if (theme) handlers['theme-changed']?.(theme);
      else handlers.notice?.('Preview only. Run the app to open things.');
    },
    hide: async () => {},
    firstFrame: async () => ({ shown: NO_SAMPLES, firstFrame: NO_SAMPLES }),
    on: async (event, handler) => {
      handlers[event] = handler as never;
    },
  };
}
