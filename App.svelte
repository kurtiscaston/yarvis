<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { connect, type Backend, type Hit, type HotkeyTimings, type Theme } from './lib/ipc';
  import { NO_SAMPLES, Samples, type Summary } from './lib/stats';

  let query = $state('');
  let hits = $state<Hit[]>([]);
  let selected = $state(0);
  let ready = $state(false);
  /** One line of feedback that replaces the footer until the next key press. */
  let notice = $state('');
  let showTimings = $state(true);

  let searchMs = $state(0);
  let keyToFrame = $state<Summary>(NO_SAMPLES);
  let hotkey = $state<HotkeyTimings>({ shown: NO_SAMPLES, firstFrame: NO_SAMPLES });

  let backend: Backend;
  let input: HTMLInputElement;
  let list: HTMLElement;
  /** Only the newest search may update the list; slower, older ones are dropped. */
  let newest = 0;
  let themeProperties: string[] = [];
  /** Where the mouse last was, or null until it has been seen since the window appeared. */
  let pointer: { x: number; y: number } | null = null;
  const keySamples = new Samples();

  onMount(async () => {
    backend = await connect();

    await backend.on('launcher-shown', (seq) => {
      input.focus();
      pointer = null;
      if (seq === null) return;
      // The first frame after the window is shown: report it for the hotkey timing.
      requestAnimationFrame(async () => {
        hotkey = await backend.firstFrame(seq);
      });
    });
    await backend.on('launcher-reset', () => {
      query = '';
      notice = '';
      void runSearch('');
    });
    await backend.on('theme-changed', (theme) => {
      applyTheme(theme);
      query = '';
      void runSearch('');
    });
    await backend.on('settings-changed', (settings) => {
      showTimings = settings.showTimings;
    });
    await backend.on('notice', (text) => {
      notice = text;
    });

    const start = await backend.bootstrap();
    applyTheme(start.theme);
    showTimings = start.settings.showTimings;
    notice = start.problems[0] ?? '';
    await runSearch('');
    ready = true;
    input.focus();
  });

  function applyTheme(theme: Theme) {
    const root = document.documentElement.style;
    for (const property of themeProperties) root.removeProperty(property);
    themeProperties = Object.keys(theme.tokens).map((token) => `--${token}`);
    for (const [token, value] of Object.entries(theme.tokens)) root.setProperty(`--${token}`, value);
    root.colorScheme = theme.appearance;
  }

  /** `typedAt` is the key press's own timestamp, when the search came from typing. */
  async function runSearch(text: string, typedAt?: number) {
    const ticket = ++newest;
    const response = await backend.search(text);
    if (ticket !== newest) return;

    hits = response.hits;
    selected = 0;
    searchMs = response.searchUs / 1000;
    await tick();
    list.scrollTop = 0;

    if (typedAt === undefined) return;
    // This callback runs as the frame that paints these results starts rendering.
    // (The callback's own argument is the display's tick time, which can be
    // earlier than the key press, so the clock is read here instead.)
    requestAnimationFrame(() => {
      keySamples.add(performance.now() - typedAt);
      keyToFrame = keySamples.summary();
    });
  }

  function onInput(event: Event) {
    notice = '';
    void runSearch(query, event.timeStamp);
  }

  async function select(index: number) {
    if (hits.length === 0) return;
    selected = (index + hits.length) % hits.length;
    await tick();
    list.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: 'nearest' });
  }

  async function activate(hit: Hit | undefined) {
    if (!hit) return;
    try {
      await backend.activate(hit.id);
    } catch (problem) {
      notice = String(problem);
    }
  }

  function onKeydown(event: KeyboardEvent) {
    const withCtrl = event.ctrlKey && !event.altKey && !event.metaKey;
    if (event.key === 'ArrowDown' || (withCtrl && event.key === 'n')) {
      void select(selected + 1);
    } else if (event.key === 'ArrowUp' || (withCtrl && event.key === 'p')) {
      void select(selected - 1);
    } else if (event.key === 'Enter') {
      void activate(hits[selected]);
    } else if (event.key === 'Escape') {
      if (query) {
        query = '';
        void runSearch('');
      } else {
        void backend.hide();
      }
    } else {
      return;
    }
    event.preventDefault();
  }

  /**
   * Follow the mouse only when it really moves. A window that appears under a
   * resting mouse, or rows that scroll under it, must not steal the selection.
   */
  function onPointerMove(event: MouseEvent, index: number) {
    const moved = pointer !== null && (event.screenX !== pointer.x || event.screenY !== pointer.y);
    pointer = { x: event.screenX, y: event.screenY };
    if (moved) selected = index;
  }

  /** Splits a title into runs of matched and unmatched characters. */
  function segments(hit: Hit): { text: string; matched: boolean }[] {
    if (hit.matched.length === 0) return [{ text: hit.title, matched: false }];
    const marks = new Set(hit.matched);
    const runs: { text: string; matched: boolean }[] = [];
    Array.from(hit.title).forEach((char, index) => {
      const matched = marks.has(index);
      const run = runs[runs.length - 1];
      if (run && run.matched === matched) run.text += char;
      else runs.push({ text: char, matched });
    });
    return runs;
  }

  const ms = (value: number) => (value < 10 ? value.toFixed(1) : value.toFixed(0));
  const pair = (summary: Summary) => (summary.count ? `${ms(summary.median)} / ${ms(summary.p95)}` : '–');
</script>

<!-- A click anywhere must not take the caret out of the search box. -->
<svelte:window onmousedown={(event) => event.target !== input && event.preventDefault()} />

<main>
  <label class="query">
    <span class="mark" aria-hidden="true"></span>
    <!-- svelte-ignore a11y_autofocus -->
    <input
      bind:this={input}
      bind:value={query}
      oninput={onInput}
      onkeydown={onKeydown}
      type="text"
      role="combobox"
      aria-label="Search apps and commands"
      aria-expanded="true"
      aria-controls="results"
      aria-activedescendant={hits.length ? `hit-${selected}` : undefined}
      placeholder="Search apps and commands"
      spellcheck="false"
      autocomplete="off"
      autocapitalize="off"
      autofocus
    />
  </label>

  <div class="results" id="results" role="listbox" aria-label="Results" bind:this={list}>
    {#each hits as hit, index (hit.id)}
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <div
        class="row"
        id="hit-{index}"
        role="option"
        tabindex="-1"
        aria-selected={index === selected}
        onmousemove={(event) => onPointerMove(event, index)}
        onclick={() => activate(hit)}
      >
        <span class="tile" aria-hidden="true">{Array.from(hit.title)[0]?.toUpperCase()}</span>
        <span class="title">
          {#each segments(hit) as run}{#if run.matched}<mark>{run.text}</mark>{:else}{run.text}{/if}{/each}
        </span>
        <span class="kind">{hit.kind}</span>
      </div>
    {:else}
      {#if ready}
        <p class="empty">Nothing matches “{query.trim()}”. If an app is missing, run “Rescan applications”.</p>
      {/if}
    {/each}
  </div>

  <footer>
    {#if notice}
      <p class="notice" role="status" title={notice}>{notice}</p>
    {:else}
      <p class="keys"><kbd>Enter</kbd> open <kbd>Esc</kbd> {query ? 'clear' : 'close'}</p>
      {#if showTimings}
        <!-- Milliseconds. Pairs are median / 95th percentile. See the README. -->
        <dl class="timings">
          <div title="Time inside the search engine for the last query">
            <dt>Search</dt><dd>{searchMs.toFixed(2)}</dd>
          </div>
          <div title="Key press to the frame that paints its results (median / 95th percentile)">
            <dt>Typing</dt><dd>{pair(keyToFrame)}</dd>
          </div>
          <div title="Hotkey to the window being shown (median / 95th percentile)">
            <dt>Shown</dt><dd>{pair(hotkey.shown)}</dd>
          </div>
          <div title="Hotkey to the first frame drawn after showing (median / 95th percentile)">
            <dt>First frame</dt><dd>{pair(hotkey.firstFrame)}</dd>
          </div>
        </dl>
      {/if}
    {/if}
  </footer>
</main>

<style>
  main {
    display: grid;
    grid-template-rows: 72px minmax(0, 1fr) 48px; /* 8 rows fit in a 484px window */
    height: 100%;
    background: var(--surface);
  }

  /* The search line. The solid mark and the selection stripe below share one
     colour: the mark is where you type, the stripe is what Enter will open. */
  .query {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 0 var(--gutter);
    border-bottom: 1px solid var(--rule);
  }

  .mark {
    flex: none;
    width: 8px;
    height: 28px;
    background: var(--caret);
  }

  input {
    flex: 1;
    min-width: 0;
    padding: 0;
    border: 0;
    outline: 0;
    background: none;
    color: inherit;
    caret-color: var(--caret);
    font: 500 26px/1.2 var(--font);
    letter-spacing: -0.015em;
  }

  input::placeholder {
    color: var(--muted);
    opacity: 0.75;
  }

  .results {
    overflow-y: auto;
    scrollbar-width: none;
    padding: 6px 0;
  }

  .row {
    display: grid;
    grid-template-columns: 28px minmax(0, 1fr) auto;
    align-items: center;
    gap: 14px;
    height: var(--row);
    padding: 0 var(--gutter);
  }

  /* Full-bleed and unrounded on purpose: the one loud thing in the window. */
  .row[aria-selected='true'] {
    background: var(--select);
    color: var(--select-text);
  }

  .tile {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border-radius: 7px;
    background: var(--tile);
    font-size: 13px;
    font-weight: 650;
  }

  .row[aria-selected='true'] .tile {
    background: color-mix(in srgb, var(--select-text) 16%, transparent);
  }

  .title {
    overflow: hidden;
    font-size: 15.5px;
    font-weight: 480;
    /* Tall enough that the underline on matched letters is not clipped. */
    line-height: 30px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  mark {
    background: none;
    color: inherit;
    text-decoration-line: underline;
    text-decoration-thickness: 2px;
    text-decoration-color: var(--caret);
    text-underline-offset: 4px;
  }

  .row[aria-selected='true'] mark {
    text-decoration-color: currentColor;
  }

  .kind {
    color: var(--muted);
    font-size: 12.5px;
  }

  .row[aria-selected='true'] .kind {
    color: inherit;
    opacity: 0.72;
  }

  .empty {
    margin: 0;
    padding: 18px var(--gutter);
    color: var(--muted);
    font-size: 14.5px;
    line-height: 1.5;
  }

  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 24px;
    padding: 0 var(--gutter);
    border-top: 1px solid var(--rule);
    color: var(--muted);
    font-size: 12px;
  }

  footer p {
    margin: 0;
  }

  /* Up to two lines, so a message that names a file path still fits. */
  .notice {
    display: -webkit-box;
    overflow: hidden;
    color: var(--text);
    font-size: 13px;
    line-height: 1.3;
    overflow-wrap: anywhere;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
  }

  .keys {
    flex: none;
  }

  kbd {
    margin: 0 3px 0 10px;
    padding: 2px 6px;
    border-radius: 5px;
    background: var(--tile);
    color: var(--text);
    font: inherit;
  }

  kbd:first-child {
    margin-left: 0;
  }

  .timings {
    display: flex;
    gap: 18px;
    margin: 0;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .timings div {
    display: flex;
    gap: 6px;
  }

  .timings dd {
    margin: 0;
    color: var(--text);
  }
</style>
