<script lang="ts">
  import CircleCheck from '@lucide/svelte/icons/circle-check'
  import ListFilter from '@lucide/svelte/icons/list-filter'
  import LoaderCircle from '@lucide/svelte/icons/loader-circle'
  import Plus from '@lucide/svelte/icons/plus'
  import Search from '@lucide/svelte/icons/search'
  import { SEARCH_CATEGORIES, type SearchCategory, type SearchResult } from '$lib/api'
  import { age, bytes, count } from '$lib/format'
  import { seedLevel, type SearchSortKey } from '$lib/search'
  import { app } from '$lib/state.svelte'
  import Logo from './Logo.svelte'

  let { compact }: { compact: boolean } = $props()

  let box = $state<HTMLInputElement>()
  let rowsEl = $state<HTMLDivElement>()

  const reply = $derived(app.searchReply)
  // the same link can come back twice in rare cases, so a counter keeps keys unique
  const keyed = $derived.by(() => {
    const seen = new Map<string, number>()
    return app.results.map((r) => {
      const base = r.info_hash ?? r.link
      const n = seen.get(base) ?? 0
      seen.set(base, n + 1)
      return { r, key: n ? `${base}#${n}` : base }
    })
  })
  // Jackett prefixes its errors with the exception type, which says nothing to a person
  const failures = $derived((reply?.failures ?? []).map((f) => ({ ...f, error: f.error.replace(/^([\w.]+Exception:\s*)+/, '') })))
  const asking = $derived(
    (app.defaults?.indexers ?? [])
      .filter((indexer) => indexer.enabled)
      .map((indexer) => indexer.name || indexer.kind)
      .join(', ') || 'your indexers'
  )

  const columns: { key: SearchSortKey; label: string; title: string }[] = [
    { key: 'size', label: 'size', title: 'size' },
    { key: 'seeds', label: 'seeds', title: 'seeds' },
    { key: 'peers', label: 'peers', title: 'peers still downloading' },
    { key: 'published', label: 'age', title: 'age' },
    { key: 'source', label: 'source', title: 'tracker' }
  ]

  export function focusBox(): void {
    box?.focus()
    box?.select()
  }

  function submit(event: SubmitEvent): void {
    event.preventDefault()
    void app.search()
  }

  function pickCategory(category: SearchCategory): void {
    if (app.searchCategory === category) return
    app.searchCategory = category
    if (app.searchText.trim() && (reply || app.searchError)) void app.search()
  }

  function known(r: SearchResult): boolean {
    return r.info_hash !== null && app.known.has(r.info_hash)
  }

  function menu(r: SearchResult, event: MouseEvent): void {
    event.preventDefault()
    open(r, event.clientX, event.clientY)
  }

  function open(r: SearchResult, x: number, y: number): void {
    app.menu = {
      x,
      y,
      items: [
        { label: known(r) ? 'show in the list' : 'add', action: () => app.pickResult(r) },
        { label: 'copy link', action: () => void app.copy(r.link, 'link') },
        {
          label: 'open page',
          disabled: !r.info_url,
          action: () => r.info_url && window.open(r.info_url, '_blank', 'noopener,noreferrer')
        }
      ]
    }
  }

  function keys(event: KeyboardEvent, at: number): void {
    const rows = [...(rowsEl?.querySelectorAll<HTMLButtonElement>('.row') ?? [])]
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault()
      const next = event.key === 'ArrowDown' ? Math.min(rows.length - 1, at + 1) : Math.max(0, at - 1)
      rows[next]?.focus()
    } else if (event.key === 'ContextMenu') {
      event.preventDefault()
      const rect = rows[at].getBoundingClientRect()
      open(app.results[at], rect.left + 40, rect.bottom)
    }
  }

  function boxKeys(event: KeyboardEvent): void {
    if (event.key === 'ArrowDown') {
      event.preventDefault()
      rowsEl?.querySelector<HTMLButtonElement>('.row')?.focus()
    }
  }

  function focusOnOpen(node: HTMLInputElement): void {
    if (!compact) node.focus()
  }

  function meta(r: SearchResult): string {
    return [bytes(r.size), r.seeds !== null ? `${count(r.seeds)} seeds` : '', age(r.published), r.source].filter(Boolean).join(' · ')
  }
</script>

<section class={['search', compact && 'compact']} aria-label="search">
  {#if !app.hasIndexers}
    <div class="empty">
      <Logo size={34} word={false} />
      <p>search asks prowlarr or jackett for torrents.</p>
      <button type="button" class="btn" onclick={() => app.openSettings('search')}><Plus />add one</button>
    </div>
  {:else}
    <form class="bar" role="search" onsubmit={submit}>
      <label class="box">
        <Search size={15} />
        <input
          class="input"
          type="search"
          placeholder="search"
          aria-label="search for torrents"
          enterkeyhint="search"
          spellcheck="false"
          autocomplete="off"
          bind:this={box}
          bind:value={app.searchText}
          onkeydown={boxKeys}
          {@attach focusOnOpen}
        />
        {#if app.searching}<LoaderCircle size={15} class="spin busy" aria-label="searching" />{/if}
      </label>
    </form>

    <div class="tools">
      <div class="chips" role="group" aria-label="category">
        {#each SEARCH_CATEGORIES as category (category)}
          <button type="button" class={['chip', app.searchCategory === category && 'on']} aria-pressed={app.searchCategory === category} onclick={() => pickCategory(category)}>
            {category}
          </button>
        {/each}
      </div>
      {#if reply && reply.results.length > 0}
        <label class="refine" title="words to keep, -word to leave out, >1GB or <500MB for size">
          <ListFilter size={13} />
          <input class="input" type="search" placeholder="refine" aria-label="refine results" spellcheck="false" bind:value={app.refine} />
        </label>
      {/if}
    </div>

    {#if app.searchError}
      <p class="error" role="alert">{app.searchError}</p>
    {/if}
    {#if failures.length}
      <p class="warn truncate" title={failures.map((f) => `${f.source}: ${f.error}`).join('\n')}>
        {failures.map((f) => `${f.source}: ${f.error}`).join(' · ')}
      </p>
    {/if}

    {#if app.searching && !reply}
      <div class="waiting" role="status"><LoaderCircle size={16} class="spin" /><span>asking {asking}…</span></div>
    {:else if reply}
      {#if app.results.length === 0}
        <p class="nothing muted">{reply.results.length ? 'nothing matches.' : 'nothing found.'}</p>
      {:else}
        {#if !compact}
          <div class="head" role="presentation">
            <span></span>
            {@render sortButton('title', 'name', 'name')}
            {#each columns as c (c.key)}
              {@render sortButton(c.key, c.label, c.title)}
            {/each}
          </div>
        {/if}
        <div class={['rows', app.searching && 'stale']} role="group" aria-label="results" aria-busy={app.searching} bind:this={rowsEl}>
          {#each keyed as { r, key }, ix (key)}
            {@const have = known(r)}
            {@const level = seedLevel(r.seeds)}
            <button
              type="button"
              class={['row', have && 'have']}
              title={have ? 'already in the list' : r.title}
              onclick={() => app.pickResult(r)}
              oncontextmenu={(e) => menu(r, e)}
              onkeydown={(e) => keys(e, ix)}
            >
              <span class="lead" aria-hidden="true">
                {#if have}<CircleCheck size={14} />{:else}<Plus size={14} class="plus" />{/if}
              </span>
              <span class="name">
                <span class="title truncate">{r.title}</span>
                {#if compact}<span class="meta num truncate">{meta(r)}</span>{/if}
              </span>
              {#if !compact}
                <span class="c num muted">{r.size ? bytes(r.size) : ''}</span>
                <span class={['c', 'num', 'seeds', level]}>{r.seeds === null ? '' : count(r.seeds)}</span>
                <span class="c num muted">{r.peers === null ? '' : count(r.peers)}</span>
                <span class="c num muted">{age(r.published)}</span>
                <span class="source truncate muted" title={r.via === r.source ? r.source : `${r.source}, through ${r.via}`}>{r.source}</span>
              {/if}
            </button>
          {/each}
        </div>
      {/if}
    {/if}
  {/if}
</section>

{#snippet sortButton(key: SearchSortKey, label: string, hint: string)}
  <button
    type="button"
    class={['sort', key !== 'title' && key !== 'source' && 'c', app.searchSort.key === key && 'on']}
    title="sort by {hint}"
    aria-pressed={app.searchSort.key === key}
    onclick={() => app.setSearchSort(key)}
  >
    {label}{#if app.searchSort.key === key}<span class="arrow" aria-hidden="true">{app.searchSort.desc ? '▾' : '▴'}</span>{/if}
  </button>
{/snippet}

<style>
  .search {
    container-type: inline-size;
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
    min-height: 0;
  }
  .bar {
    padding: 12px 12px 0;
    flex: none;
  }
  .box {
    position: relative;
    display: flex;
    align-items: center;
    max-width: 640px;
    color: var(--text-weaker);
  }
  .box > :global(svg:first-child) {
    position: absolute;
    left: 11px;
    pointer-events: none;
  }
  .box :global(.busy) {
    position: absolute;
    right: 34px;
    color: var(--yolk);
  }
  .box .input {
    height: 36px;
    padding-left: 34px;
    font-size: 14px;
  }
  .tools {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px 16px;
    padding: 10px 12px;
    flex: none;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .chip {
    height: 26px;
    padding: 0 10px;
    border: 1px solid var(--border-weak);
    border-radius: var(--radius);
    background: none;
    color: var(--text-weak);
    font-size: 12px;
    cursor: pointer;
  }
  .chip:hover {
    color: var(--text-strong);
    background: var(--bg-weak);
  }
  .chip.on {
    border-color: var(--yolk);
    background: var(--yolk-dim);
    color: var(--text-strong);
  }
  .refine {
    position: relative;
    display: flex;
    align-items: center;
    width: 220px;
    margin-left: auto;
    color: var(--text-weaker);
  }
  .refine :global(svg) {
    position: absolute;
    left: 9px;
    pointer-events: none;
  }
  .refine .input {
    height: 28px;
    padding-left: 28px;
    border-color: var(--border-weak);
    background: transparent;
    font-size: 12px;
  }
  .error,
  .warn {
    padding: 0 12px 8px;
    font-size: 12px;
  }
  .error {
    color: var(--danger);
  }
  .warn {
    color: var(--warning);
  }
  .waiting {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    flex: 1;
    color: var(--text-weak);
  }
  .waiting :global(svg) {
    color: var(--yolk);
  }
  .nothing {
    padding: 32px;
    text-align: center;
  }

  .head,
  .row {
    display: grid;
    grid-template-columns: 34px minmax(0, 1fr) 76px 60px 56px 46px minmax(0, 150px);
    align-items: center;
    column-gap: 10px;
    padding-right: 12px;
  }
  .head {
    height: 26px;
    flex: none;
    border-top: 1px solid var(--border-weak);
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
  .rows {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }
  .rows.stale {
    opacity: 0.55;
  }
  .row {
    width: 100%;
    height: 38px;
    border: 0;
    border-bottom: 1px solid color-mix(in oklch, var(--border-weak) 55%, transparent);
    background: none;
    color: inherit;
    text-align: left;
    cursor: pointer;
    content-visibility: auto;
    contain-intrinsic-size: auto 38px;
  }
  .row:hover,
  .row:focus-visible {
    background: var(--bg-weak);
    outline: none;
  }
  .row:focus-visible {
    box-shadow: inset 2px 0 0 var(--focus);
  }
  .lead {
    display: flex;
    justify-content: center;
    color: var(--text-weaker);
  }
  .lead :global(.plus) {
    opacity: 0;
  }
  .row:hover .lead :global(.plus),
  .row:focus-visible .lead :global(.plus) {
    opacity: 1;
  }
  .row.have .lead {
    color: var(--success);
  }
  .name {
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-width: 0;
  }
  .title {
    color: var(--text-strong);
  }
  .row.have .title {
    color: var(--text);
  }
  .seeds.good {
    color: var(--success);
  }
  .seeds.low {
    color: var(--warning);
  }
  .seeds.none {
    color: var(--text-weaker);
  }
  .source {
    font-size: 12px;
  }
  .meta {
    color: var(--text-weak);
    font-size: 12px;
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

  @container (max-width: 760px) {
    .head,
    .row {
      grid-template-columns: 34px minmax(0, 1fr) 76px 60px 46px;
    }
    .head > :nth-child(5),
    .row > :nth-child(5),
    .head > :nth-child(7),
    .row > :nth-child(7) {
      display: none;
    }
  }
  .compact .bar {
    padding: 10px 8px 0;
  }
  .compact .tools {
    padding: 8px;
  }
  .compact .chips {
    flex-wrap: nowrap;
    overflow-x: auto;
    scrollbar-width: none;
  }
  .compact .refine {
    width: 100%;
    margin-left: 0;
  }
  .compact .row {
    grid-template-columns: 34px minmax(0, 1fr);
    height: 56px;
    padding-right: 10px;
    contain-intrinsic-size: auto 56px;
  }
  .compact .lead :global(.plus) {
    opacity: 0.5;
  }
</style>
