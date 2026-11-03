<script lang="ts">
  import Info from '@lucide/svelte/icons/info'
  import Pause from '@lucide/svelte/icons/pause'
  import Play from '@lucide/svelte/icons/play'
  import Plus from '@lucide/svelte/icons/plus'
  import Search from '@lucide/svelte/icons/search'
  import Trash from '@lucide/svelte/icons/trash-2'
  import X from '@lucide/svelte/icons/x'
  import type { Torrent } from '$lib/api'
  import { bytes, eta, percent, ratio, speed } from '$lib/format'
  import { openTorrentMenu } from '$lib/menus'
  import { app, type SortKey } from '$lib/state.svelte'
  import { barColor, isRunning, looks } from '$lib/states'
  import Logo from './Logo.svelte'

  let { compact }: { compact: boolean } = $props()

  const ROW = $derived(compact ? 64 : 40)
  const OVERSCAN = 8

  let scroller = $state<HTMLDivElement>()
  let filterInput = $state<HTMLInputElement>()
  let scrollTop = $state(0)
  let height = $state(600)

  const rows = $derived(app.visible)
  const start = $derived(Math.max(0, Math.floor(scrollTop / ROW) - OVERSCAN))
  const end = $derived(Math.min(rows.length, Math.ceil((scrollTop + height) / ROW) + OVERSCAN))
  const slice = $derived(rows.slice(start, end))

  const chosen = $derived(app.torrents.filter((t) => app.selected.has(t.id)))
  const anyRunning = $derived(chosen.some(isRunning))
  const anyStopped = $derived(chosen.some((t) => !isRunning(t)))

  const columns: { key: SortKey; label: string; title: string }[] = [
    { key: 'size', label: 'size', title: 'size' },
    { key: 'progress', label: 'done', title: 'progress' },
    { key: 'down', label: '↓', title: 'download speed' },
    { key: 'up', label: '↑', title: 'upload speed' },
    { key: 'eta', label: 'eta', title: 'time left' },
    { key: 'ratio', label: 'ratio', title: 'uploaded ÷ downloaded' },
    { key: 'peers', label: 'peers', title: 'connected peers' }
  ]

  export function focusFilter(): void {
    filterInput?.focus()
    filterInput?.select()
  }

  function reveal(): void {
    if (!scroller || !app.focused) return
    const ix = rows.findIndex((t) => t.id === app.focused)
    if (ix < 0) return
    const top = ix * ROW
    if (top < scroller.scrollTop) scroller.scrollTop = top
    else if (top + ROW > scroller.scrollTop + scroller.clientHeight) scroller.scrollTop = top + ROW - scroller.clientHeight
  }

  function rowClick(t: Torrent, event: MouseEvent): void {
    app.click(t.id, event)
    if (compact && !event.ctrlKey && !event.metaKey && !event.shiftKey) app.openDetails(t.id)
  }

  function rowMenu(t: Torrent, event: MouseEvent): void {
    event.preventDefault()
    if (!app.selected.has(t.id)) app.click(t.id, { ctrlKey: false, metaKey: false, shiftKey: false })
    openTorrentMenu(event.clientX, event.clientY)
  }

  function keys(event: KeyboardEvent): void {
    const mod = event.ctrlKey || event.metaKey
    switch (event.key) {
      case 'ArrowDown':
      case 'ArrowUp':
        event.preventDefault()
        app.move(event.key === 'ArrowDown' ? 1 : -1, event.shiftKey)
        reveal()
        break
      case 'Home':
      case 'End':
        event.preventDefault()
        app.move(event.key === 'Home' ? -Infinity : Infinity, event.shiftKey)
        reveal()
        break
      case 'Enter':
        if (app.focused) app.openDetails(app.focused)
        break
      case ' ':
        event.preventDefault()
        app.toggle(app.targets())
        break
      case 'Delete':
      case 'Backspace':
        event.preventDefault()
        if (event.shiftKey) app.remove(app.targets(), true)
        else app.askRemove(app.targets())
        break
      case 'a':
        if (mod) {
          event.preventDefault()
          app.selectAll()
        }
        break
      case 'ContextMenu': {
        event.preventDefault()
        const rect = scroller?.getBoundingClientRect()
        if (rect) openTorrentMenu(rect.left + 40, rect.top + 40)
        break
      }
    }
  }

  function title(t: Torrent): string {
    const look = looks[t.state]
    return t.error ? `${look.label}: ${t.error}` : look.label
  }
</script>

<section class={['list', compact && 'compact']} aria-label="torrents">
  <div class="toolbar">
    <label class="filter">
      <Search size={14} />
      <input
        class="input"
        type="search"
        placeholder="filter"
        aria-label="filter torrents by name"
        spellcheck="false"
        bind:this={filterInput}
        bind:value={app.query}
        onkeydown={(e) => {
          if (e.key === 'Escape') {
            app.query = ''
            filterInput?.blur()
          } else if (e.key === 'ArrowDown') {
            e.preventDefault()
            scroller?.focus()
            app.move(1, false)
          }
        }}
      />
    </label>
    {#if app.selected.size > 0}
      <div class="selection" role="toolbar" aria-label="selected torrents">
        <span class="muted num">{app.selected.size}</span>
        <button type="button" class="btn ghost icon" aria-label="resume" title="resume" disabled={!anyStopped} onclick={() => app.resume(app.targets())}>
          <Play />
        </button>
        <button type="button" class="btn ghost icon" aria-label="pause" title="pause" disabled={!anyRunning} onclick={() => app.pause(app.targets())}>
          <Pause />
        </button>
        {#if app.selected.size === 1}
          <button type="button" class="btn ghost icon" aria-label="details" title="details" onclick={() => app.focused && app.openDetails(app.focused)}>
            <Info />
          </button>
        {/if}
        <button type="button" class="btn ghost icon" aria-label="remove" title="remove" onclick={() => app.askRemove(app.targets())}>
          <Trash />
        </button>
        <button type="button" class="btn ghost icon" aria-label="clear selection" title="clear selection" onclick={() => app.clearSelection()}>
          <X />
        </button>
      </div>
    {/if}
  </div>

  {#if app.torrents.length === 0}
    <div class={['empty', app.snapshot === null && 'waiting']}>
      {#if app.snapshot !== null}
        <Logo size={34} word={false} />
        <p>drop a .torrent file here, or paste a magnet link.</p>
        <button type="button" class="btn" onclick={() => app.openAdd()}><Plus />add torrent</button>
      {/if}
    </div>
  {:else}
    <div class="head" role="presentation">
      <span></span>
      {@render sortButton('name', 'name', 'name')}
      {#each columns as c (c.key)}
        {@render sortButton(c.key, c.label, c.title)}
      {/each}
    </div>
    <div
      class="scroller"
      role="grid"
      tabindex="0"
      aria-label="torrents"
      aria-multiselectable="true"
      aria-rowcount={rows.length}
      aria-activedescendant={app.focused ? `row-${app.focused}` : undefined}
      bind:this={scroller}
      bind:clientHeight={height}
      onscroll={() => (scrollTop = scroller?.scrollTop ?? 0)}
      onkeydown={keys}
    >
      {#if rows.length === 0}
        <p class="nothing muted">nothing matches.</p>
      {/if}
      <div class="spacer" style:height="{rows.length * ROW}px">
        <div class="window" style:transform="translateY({start * ROW}px)">
          {#each slice as t, i (t.id)}
            {@const look = looks[t.state]}
            <!-- svelte-ignore a11y_click_events_have_key_events (the grid handles the keyboard for every row) -->
            <div
              id="row-{t.id}"
              class={['row', app.selected.has(t.id) && 'selected', app.focused === t.id && 'focused']}
              role="row"
              tabindex="-1"
              aria-rowindex={start + i + 1}
              aria-selected={app.selected.has(t.id)}
              style:height="{ROW}px"
              onclick={(e) => rowClick(t, e)}
              ondblclick={() => app.openDetails(t.id)}
              oncontextmenu={(e) => rowMenu(t, e)}
            >
              <span class="state" role="gridcell" title={title(t)} style:color={look.color}>
                <look.icon size={14} class={look.spin ? 'spin' : undefined} />
              </span>
              <span class="name" role="gridcell">
                <span class="title truncate">{t.name}</span>
                <span class="bar" aria-hidden="true"><span style:width="{t.progress * 100}%" style:background={barColor(t)}></span></span>
                {#if compact}
                  <span class="meta num truncate">
                    {#if t.state === 'error'}
                      <span class="err">{t.error ?? 'error'}</span>
                    {:else if t.state === 'metadata'}
                      {look.label}
                    {:else}
                      {t.progress < 1 ? `${bytes(t.done)} of ${bytes(t.size)}` : bytes(t.size)}
                      {#if t.down}<span class="d"> ↓ {speed(t.down)}</span>{/if}
                      {#if t.up}<span class="u"> ↑ {speed(t.up)}</span>{/if}
                      {#if eta(t.eta)}<span> · {eta(t.eta)}</span>{/if}
                    {/if}
                  </span>
                {/if}
              </span>
              {#if !compact}
                <span class="c num muted" role="gridcell">{t.size ? bytes(t.size) : ''}</span>
                <span class="c num" role="gridcell">{t.state === 'metadata' ? '' : percent(t.progress)}</span>
                <span class="c num d" role="gridcell">{speed(t.down)}</span>
                <span class="c num u" role="gridcell">{speed(t.up)}</span>
                <span class="c num muted" role="gridcell">{eta(t.eta)}</span>
                <span class="c num muted" role="gridcell">{t.size ? ratio(t.ratio) : ''}</span>
                <span class="c num muted" role="gridcell" title="{t.seeds} seeds of {t.peers} peers">{t.peers || ''}</span>
              {/if}
            </div>
          {/each}
        </div>
      </div>
    </div>
  {/if}
</section>

{#snippet sortButton(key: SortKey, label: string, hint: string)}
  <button
    type="button"
    class={['sort', key !== 'name' && 'c', app.sort.key === key && 'on']}
    title="sort by {hint}"
    aria-pressed={app.sort.key === key}
    onclick={() => app.setSort(key)}
  >
    {label}{#if app.sort.key === key}<span class="arrow" aria-hidden="true">{app.sort.desc ? '▾' : '▴'}</span>{/if}
  </button>
{/snippet}

<style>
  .list {
    container-type: inline-size;
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
    min-height: 0;
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 44px;
    padding: 0 10px;
    flex: none;
  }
  .filter {
    position: relative;
    display: flex;
    align-items: center;
    flex: 1;
    max-width: 360px;
    color: var(--text-weaker);
  }
  .filter :global(svg) {
    position: absolute;
    left: 9px;
    pointer-events: none;
  }
  .filter .input {
    height: 30px;
    padding-left: 30px;
    border-color: var(--border-weak);
    background: transparent;
  }
  .filter .input:focus {
    border-color: var(--border);
  }
  .selection {
    display: flex;
    align-items: center;
    gap: 2px;
    margin-left: auto;
  }
  .selection .muted {
    margin-right: 6px;
    font-size: 12px;
  }

  .head,
  .row {
    display: grid;
    grid-template-columns: 34px minmax(0, 1fr) 76px 52px 84px 84px 62px 48px 46px;
    align-items: center;
    padding-right: 12px;
  }
  .head {
    height: 26px;
    flex: none;
    border-bottom: 1px solid var(--border-weak);
  }
  .sort {
    width: 100%;
    height: 26px;
    padding: 0;
    border: 0;
    background: none;
    color: var(--text-weaker);
    font-size: 12px;
    text-align: left;
    white-space: nowrap;
    cursor: pointer;
  }
  .sort:hover,
  .sort.on {
    color: var(--text-strong);
  }
  .arrow {
    margin-left: 3px;
  }
  .c {
    text-align: right;
    white-space: nowrap;
    overflow: hidden;
  }
  .scroller {
    position: relative;
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overflow-x: hidden;
  }
  .scroller:focus {
    outline: none;
  }
  .scroller:focus-visible .row.focused {
    box-shadow: inset 2px 0 0 var(--focus);
  }
  .spacer {
    position: relative;
  }
  .window {
    position: absolute;
    inset: 0 0 auto 0;
    will-change: transform;
  }
  .row {
    border-bottom: 1px solid color-mix(in oklch, var(--border-weak) 55%, transparent);
    cursor: default;
  }
  .row:hover {
    background: var(--bg-weak);
  }
  .row.selected {
    background: var(--bg-selected);
  }
  .row.selected .muted {
    color: var(--text);
  }
  .state {
    display: flex;
    justify-content: center;
  }
  .name {
    display: flex;
    flex-direction: column;
    gap: 5px;
    min-width: 0;
    padding-right: 16px;
  }
  .title {
    color: var(--text-strong);
    line-height: 1.2;
  }
  .bar {
    display: block;
    height: 3px;
    border-radius: 3px;
    background: var(--border-weak);
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    border-radius: 3px;
    transition: width 0.4s ease;
  }
  .d {
    color: var(--yolk);
  }
  .u {
    color: var(--success);
  }
  .empty {
    display: flex;
    flex: 1;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 14px;
    padding: 24px;
    color: var(--text-weak);
    text-align: center;
  }
  .empty :global(.mark) {
    fill: var(--text-weaker);
  }
  .nothing {
    padding: 24px;
    text-align: center;
  }
  .err {
    color: var(--danger);
  }

  @container (max-width: 900px) {
    .head,
    .row {
      grid-template-columns: 34px minmax(0, 1fr) 76px 52px 84px 84px 62px;
    }
    .head > :nth-child(n + 8),
    .row > :nth-child(n + 8) {
      display: none;
    }
  }
  .compact .head {
    display: none;
  }
  .compact .row {
    grid-template-columns: 38px minmax(0, 1fr);
    padding-right: 10px;
  }
  .compact .name {
    gap: 6px;
    padding-right: 0;
  }
  .compact .title {
    line-height: 1.3;
  }
  .meta {
    color: var(--text-weak);
    font-size: 12px;
    line-height: 1.2;
  }
  .compact .toolbar {
    padding: 0 8px;
  }
  .compact .filter {
    max-width: none;
  }
</style>
