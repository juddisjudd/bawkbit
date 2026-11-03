<script lang="ts">
  import ArrowDown from '@lucide/svelte/icons/arrow-down'
  import ArrowUp from '@lucide/svelte/icons/arrow-up'
  import Funnel from '@lucide/svelte/icons/funnel'
  import Plus from '@lucide/svelte/icons/plus'
  import Search from '@lucide/svelte/icons/search'
  import SettingsIcon from '@lucide/svelte/icons/settings'
  import Turtle from '@lucide/svelte/icons/turtle'
  import { bytes, speed } from '$lib/format'
  import { app } from '$lib/state.svelte'
  import Logo from './Logo.svelte'

  const W = 72
  const H = 20
  const SAMPLES = 90

  const totals = $derived(app.snapshot?.totals)
  const history = $derived(app.snapshot?.history.slice(-SAMPLES) ?? [])

  const lines = $derived.by(() => {
    if (history.length < 2) return { down: '', up: '' }
    const max = Math.max(1, ...history.map(([down, up]) => Math.max(down, up)))
    const step = W / (SAMPLES - 1)
    const offset = W - (history.length - 1) * step
    const line = (pick: 0 | 1) =>
      history.map((sample, ix) => `${(offset + ix * step).toFixed(1)},${(H - 1 - (sample[pick] / max) * (H - 2)).toFixed(1)}`).join(' ')
    return { down: line(0), up: line(1) }
  })

  const limitNote = (limit: number) => (limit ? `, limit ${bytes(limit)}/s` : '')
</script>

<header>
  <button class="brand" type="button" onclick={() => (app.view = 'list')} aria-label="torrents">
    <Logo size={14} />
  </button>

  <div class="right">
    {#if app.live === 'reconnecting'}
      <span class="dot pulse" style:background="var(--warning)" title="reconnecting to bawkbit…" role="status" aria-label="reconnecting"></span>
    {/if}

    {#if totals}
      <div class="speeds" title="download and upload, last {SAMPLES} seconds">
        <svg width={W} height={H} viewBox="0 0 {W} {H}" aria-hidden="true">
          <polyline points={lines.up} class="up" />
          <polyline points={lines.down} class="down" />
        </svg>
        <span class="speed num" title="downloading{limitNote(totals.down_limit)}">
          <ArrowDown size={13} class="arrow down" />
          <span class={['value', totals.down_limit > 0 && 'limited']}>{speed(totals.down) || '0 B/s'}</span>
        </span>
        <span class="speed num secondary" title="uploading{limitNote(totals.up_limit)}">
          <ArrowUp size={13} class="arrow up" />
          <span class={['value', totals.up_limit > 0 && 'limited']}>{speed(totals.up) || '0 B/s'}</span>
        </span>
      </div>

      <button
        type="button"
        class={['btn', 'ghost', 'icon', totals.alt_speed && 'on']}
        aria-pressed={totals.alt_speed}
        aria-label="slow mode"
        title={totals.alt_speed ? 'slow mode is on: alternative speed limits apply' : 'slow mode: use the alternative speed limits'}
        onclick={() => app.setAltSpeed(!totals.alt_speed)}
      >
        <Turtle />
      </button>
    {/if}

    <button
      type="button"
      class={['btn', 'ghost', 'icon', 'mobile', app.view === 'search' && 'active']}
      aria-label="search"
      aria-pressed={app.view === 'search'}
      onclick={() => (app.view === 'search' ? (app.view = 'list') : app.openSearch())}
    >
      <Search />
    </button>
    <button
      type="button"
      class={['btn', 'ghost', 'icon', 'filters', (app.status !== 'all' || app.category || app.tag) && 'on']}
      aria-label="filters"
      aria-expanded={app.filtersOpen}
      onclick={() => (app.filtersOpen = !app.filtersOpen)}
    >
      <Funnel />
    </button>
    <button type="button" class="btn ghost icon" aria-label="add torrent" title="add torrent" onclick={() => app.openAdd()}>
      <Plus />
    </button>
    <button
      type="button"
      class={['btn', 'ghost', 'icon', app.view === 'settings' && 'active']}
      aria-label="settings"
      title="settings"
      aria-pressed={app.view === 'settings'}
      onclick={() => (app.view = app.view === 'settings' ? 'list' : 'settings')}
    >
      <SettingsIcon />
    </button>
  </div>
</header>

<style>
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    height: var(--header);
    padding: 0 8px 0 14px;
    border-bottom: 1px solid var(--border-weak);
    flex: none;
  }
  .brand {
    display: flex;
    align-items: center;
    height: 30px;
    padding: 0 4px;
    margin-left: -4px;
    border: 0;
    border-radius: var(--radius);
    background: none;
    cursor: pointer;
  }
  .right {
    display: flex;
    align-items: center;
    gap: 2px;
    min-width: 0;
  }
  .speeds {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-right: 10px;
    font-size: 12px;
  }
  svg polyline {
    fill: none;
    stroke-width: 1.25;
    stroke-linejoin: round;
  }
  .down {
    stroke: var(--yolk);
  }
  .up {
    stroke: var(--success);
    opacity: 0.8;
  }
  .speed {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    color: var(--text-strong);
    white-space: nowrap;
  }
  .value {
    min-width: 8ch;
  }
  .value.limited {
    text-decoration: underline dotted var(--text-weaker);
    text-underline-offset: 3px;
  }
  .speed :global(.arrow.down) {
    color: var(--yolk);
  }
  .speed :global(.arrow.up) {
    color: var(--success);
  }
  .btn.active {
    color: var(--text-strong);
    background: var(--bg-weak-hover);
  }
  .filters,
  .mobile {
    display: none;
  }
  @media (max-width: 720px) {
    header {
      padding: 0 4px 0 10px;
      gap: 6px;
    }
    .brand :global(.logo > span) {
      display: none;
    }
    svg {
      display: none;
    }
    .speeds {
      gap: 8px;
      margin-right: 4px;
    }
    .value {
      min-width: 0;
    }
    .filters,
    .mobile {
      display: inline-flex;
    }
  }
  @media (max-width: 380px) {
    .secondary {
      display: none;
    }
  }
</style>
