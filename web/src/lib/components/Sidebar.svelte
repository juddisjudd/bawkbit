<script lang="ts">
  import Activity from '@lucide/svelte/icons/activity'
  import ArrowDown from '@lucide/svelte/icons/arrow-down'
  import ArrowUp from '@lucide/svelte/icons/arrow-up'
  import Check from '@lucide/svelte/icons/check'
  import CircleAlert from '@lucide/svelte/icons/circle-alert'
  import Folder from '@lucide/svelte/icons/folder'
  import HardDrive from '@lucide/svelte/icons/hard-drive'
  import Layers from '@lucide/svelte/icons/layers'
  import Network from '@lucide/svelte/icons/network'
  import Pause from '@lucide/svelte/icons/pause'
  import Tag from '@lucide/svelte/icons/tag'
  import type { Component } from 'svelte'
  import { bytes, count } from '$lib/format'
  import { app, type StatusFilter } from '$lib/state.svelte'

  const statuses: { id: StatusFilter; label: string; icon: Component<{ size?: number }> }[] = [
    { id: 'all', label: 'all', icon: Layers },
    { id: 'downloading', label: 'downloading', icon: ArrowDown },
    { id: 'seeding', label: 'seeding', icon: ArrowUp },
    { id: 'done', label: 'done', icon: Check },
    { id: 'paused', label: 'paused', icon: Pause },
    { id: 'active', label: 'active', icon: Activity },
    { id: 'error', label: 'error', icon: CircleAlert }
  ]

  const totals = $derived(app.snapshot?.totals)
  const categories = $derived(app.snapshot?.categories ?? [])
  const tags = $derived(app.snapshot?.tags ?? [])

  function categoryMenu(event: MouseEvent, name: string, path: string): void {
    event.preventDefault()
    app.menu = {
      x: event.clientX,
      y: event.clientY,
      items: [
        {
          label: 'change folder…',
          action: () =>
            (app.prompt = {
              placeholder: 'folder for this category; empty for the default',
              value: path,
              submit: 'save',
              run: (value) => void app.act({ action: 'save_category', name, save_path: value })
            })
        },
        {
          label: 'remove category',
          danger: true,
          action: () => {
            if (app.category === name) app.filterBy('all')
            void app.act({ action: 'remove_category', name })
          }
        }
      ]
    }
  }

  function tagMenu(event: MouseEvent, tag: string): void {
    event.preventDefault()
    app.menu = {
      x: event.clientX,
      y: event.clientY,
      items: [
        {
          label: 'remove tag',
          danger: true,
          action: () => {
            if (app.tag === tag) app.filterBy('all')
            void app.act({ action: 'remove_tag', tag })
          }
        }
      ]
    }
  }
</script>

{#if app.filtersOpen}
  <div class="scrim" role="presentation" onclick={() => (app.filtersOpen = false)}></div>
{/if}

<nav class={['sidebar', app.filtersOpen && 'open']} aria-label="filters">
  <div class="group">
    {#each statuses as s (s.id)}
      {@const n = app.counts.statuses[s.id]}
      {#if s.id !== 'error' || n > 0}
        {@const active = app.status === s.id && !app.category && !app.tag}
        <button type="button" class={['item', active && 'active', s.id === 'error' && 'danger']} aria-current={active} onclick={() => app.filterBy(s.id)}>
          <s.icon size={14} />
          <span class="label">{s.label}</span>
          {#if n > 0}<span class="n num">{count(n)}</span>{/if}
        </button>
      {/if}
    {/each}
  </div>

  {#if categories.length}
    <div class="group">
      {#each categories as c (c.name)}
        {@const active = app.category === c.name}
        <button
          type="button"
          class={['item', active && 'active']}
          aria-current={active}
          title={c.save_path || undefined}
          onclick={() => (active ? app.filterBy('all') : app.filterBy('all', c.name))}
          oncontextmenu={(e) => categoryMenu(e, c.name, c.save_path)}
        >
          <Folder size={14} />
          <span class="label truncate">{c.name}</span>
          {#if app.counts.categories.get(c.name)}<span class="n num">{app.counts.categories.get(c.name)}</span>{/if}
        </button>
      {/each}
    </div>
  {/if}

  {#if tags.length}
    <div class="group">
      {#each tags as tag (tag)}
        {@const active = app.tag === tag}
        <button
          type="button"
          class={['item', active && 'active']}
          aria-current={active}
          onclick={() => (active ? app.filterBy('all') : app.filterBy('all', null, tag))}
          oncontextmenu={(e) => tagMenu(e, tag)}
        >
          <Tag size={14} />
          <span class="label truncate">{tag}</span>
          {#if app.counts.tags.get(tag)}<span class="n num">{app.counts.tags.get(tag)}</span>{/if}
        </button>
      {/each}
    </div>
  {/if}

  {#if totals}
    <div class="footer num">
      {#if totals.free_space !== null}
        <span title="free space in the download folder"><HardDrive size={13} />{bytes(totals.free_space)}</span>
      {/if}
      <span
        title={totals.port ? `${count(totals.dht_nodes)} DHT nodes, listening on port ${totals.port}` : `${count(totals.dht_nodes)} DHT nodes, not listening for incoming peers`}
        class={[!totals.port && 'warn']}
      >
        <Network size={13} />{count(totals.dht_nodes)}
      </span>
    </div>
  {/if}
</nav>

<style>
  .sidebar {
    display: flex;
    flex-direction: column;
    width: 200px;
    flex: none;
    padding: 8px;
    border-right: 1px solid var(--border-weak);
    overflow-y: auto;
  }
  .group {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .group + .group {
    margin-top: 8px;
    padding-top: 8px;
    border-top: 1px solid var(--border-weak);
  }
  .item {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 30px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--radius);
    background: none;
    color: var(--text);
    text-align: left;
    cursor: pointer;
  }
  .item :global(svg) {
    flex: none;
    color: var(--icon);
  }
  .item:hover {
    background: var(--bg-weak);
  }
  .item.active {
    background: var(--bg-weak-hover);
    color: var(--text-strong);
  }
  .item.active :global(svg) {
    color: var(--text-strong);
  }
  .item.danger,
  .item.danger :global(svg) {
    color: var(--danger);
  }
  .label {
    flex: 1;
  }
  .n {
    color: var(--text-weak);
    font-size: 12px;
  }
  .footer {
    display: flex;
    gap: 14px;
    margin-top: auto;
    padding: 12px 10px 4px;
    color: var(--text-weaker);
    font-size: 12px;
  }
  .footer span {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }
  .footer .warn {
    color: var(--warning);
  }
  .scrim {
    display: none;
  }
  @media (max-width: 720px) {
    .sidebar {
      display: none;
    }
    .sidebar.open {
      position: fixed;
      top: var(--header);
      left: 0;
      right: 0;
      z-index: 60;
      display: flex;
      width: auto;
      max-height: 70vh;
      padding: 8px 10px 12px;
      border-right: 0;
      border-bottom: 1px solid var(--border-weak);
      background: var(--bg);
      box-shadow: var(--shadow);
    }
    .item {
      height: 40px;
    }
    .scrim {
      display: block;
      position: fixed;
      inset: var(--header) 0 0 0;
      z-index: 59;
      background: oklch(0 0 0 / 0.35);
    }
  }
</style>
