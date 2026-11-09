<script lang="ts">
  import Copy from '@lucide/svelte/icons/copy'
  import Files from '@lucide/svelte/icons/files'
  import Info from '@lucide/svelte/icons/info'
  import Pause from '@lucide/svelte/icons/pause'
  import Play from '@lucide/svelte/icons/play'
  import RadioTower from '@lucide/svelte/icons/radio-tower'
  import Trash from '@lucide/svelte/icons/trash-2'
  import Users from '@lucide/svelte/icons/users'
  import X from '@lucide/svelte/icons/x'
  import type { Component } from 'svelte'
  import { api, type Details, type TrackerReport } from '$lib/api'
  import { bytes, count, date, eta, percent, ratio, speed, splitPath } from '$lib/format'
  import { app, type DetailsTab } from '$lib/state.svelte'
  import { isRunning, looks } from '$lib/states'
  import PieceMap from './PieceMap.svelte'

  let { compact }: { compact: boolean } = $props()

  const tabs: { id: DetailsTab; label: string; icon: Component<{ size?: number }> }[] = [
    { id: 'overview', label: 'overview', icon: Info },
    { id: 'files', label: 'files', icon: Files },
    { id: 'peers', label: 'peers', icon: Users },
    { id: 'trackers', label: 'trackers', icon: RadioTower }
  ]

  let details = $state.raw<Details | null>(null)
  let error = $state('')
  let height = $state(Number(localStorage.getItem('bawkbit.details-height')) || 300)
  let pending = $state.raw<{ id: string; files: Set<number> } | null>(null)
  let saveTimer: ReturnType<typeof setTimeout> | null = null
  let trackerText = $state('')
  let trackerBusy = $state(false)

  const torrent = $derived(app.current)
  const shown = $derived(details && details.id === app.focused ? details : null)
  const ownPending = $derived(pending && pending.id === app.focused ? pending.files : null)
  const included = $derived(ownPending ?? new Set(shown?.files.filter((f) => f.included).map((f) => f.index) ?? []))

  $effect(() => {
    const id = app.focused
    if (!id) return
    let alive = true
    const load = async () => {
      try {
        const next = await api.details(id)
        if (alive) {
          details = next
          error = ''
        }
      } catch (err) {
        if (alive) error = (err as Error).message
      }
    }
    void load()
    const timer = setInterval(load, 1500)
    return () => {
      alive = false
      clearInterval(timer)
    }
  })

  function close(): void {
    app.detailsOpen = false
  }

  function toggleFile(index: number): void {
    const next = new Set(included)
    if (next.has(index)) next.delete(index)
    else next.add(index)
    setFiles(next)
  }

  function toggleAll(): void {
    if (!shown) return
    const all = included.size === shown.files.length
    setFiles(all ? new Set() : new Set(shown.files.map((f) => f.index)))
  }

  function setFiles(next: Set<number>): void {
    const id = app.focused
    if (!id) return
    pending = { id, files: next }
    if (saveTimer) clearTimeout(saveTimer)
    if (!next.size) return
    saveTimer = setTimeout(async () => {
      saveTimer = null
      const ok = await app.act({ action: 'set_files', id, files: [...next].sort((a, b) => a - b) })
      if (ok) {
        try {
          details = await api.details(id)
        } catch {
          // the next poll will catch up
        }
      }
      if (pending?.files === next) pending = null
    }, 600)
  }

  async function refresh(id: string): Promise<void> {
    try {
      details = await api.details(id)
    } catch {
      // the next poll will catch up
    }
  }

  async function addTrackers(event: SubmitEvent): Promise<void> {
    event.preventDefault()
    const id = shown?.id
    if (!id || trackerBusy) return
    trackerBusy = true
    if (await app.addTrackers(id, trackerText)) {
      trackerText = ''
      await refresh(id)
    }
    trackerBusy = false
  }

  async function removeTracker(url: string): Promise<void> {
    if (!shown || trackerBusy) return
    const id = shown.id
    trackerBusy = true
    if (await app.removeTracker(id, url, shown.trackers)) await refresh(id)
    trackerBusy = false
  }

  async function addListTrackers(): Promise<void> {
    if (!shown || trackerBusy) return
    const id = shown.id
    trackerBusy = true
    if (await app.act({ action: 'add_list_trackers', ids: [id] })) await refresh(id)
    trackerBusy = false
  }

  const regions = typeof Intl.DisplayNames === 'function' ? new Intl.DisplayNames(undefined, { type: 'region' }) : null

  function countryName(code: string): string {
    try {
      return regions?.of(code) ?? code
    } catch {
      return code
    }
  }

  function ago(unix: number): string {
    const minutes = Math.max(1, Math.round((Date.now() / 1000 - unix) / 60))
    if (minutes < 60) return `${minutes}m ago`
    if (minutes < 60 * 48) return `${Math.round(minutes / 60)}h ago`
    return `${Math.round(minutes / 1440)}d ago`
  }

  /** The one-word state and the rest of what a tracker last said. */
  function trackerSaid(report: TrackerReport | undefined): { label: string; text: string } {
    if (!report || report.status === 'waiting') return { label: 'waiting', text: 'not asked yet' }
    const when = report.updated ? ` · ${ago(report.updated)}` : ''
    if (report.status === 'working') {
      const swarm = report.seeds !== null && report.leechers !== null ? `${report.seeds} seeds · ${report.leechers} leechers` : `${report.peers} peers`
      return { label: 'working', text: swarm + when }
    }
    return { label: 'failing', text: (report.message ?? 'no answer') + when }
  }

  function trackerKeys(event: KeyboardEvent): void {
    if (event.key === 'Enter' && (event.ctrlKey || event.metaKey)) {
      event.preventDefault()
      ;(event.currentTarget as HTMLTextAreaElement).form?.requestSubmit()
    }
  }

  function startResize(event: PointerEvent): void {
    const target = event.currentTarget as HTMLElement
    target.setPointerCapture(event.pointerId)
    const startY = event.clientY
    const startHeight = height
    const move = (e: PointerEvent) => {
      height = Math.max(160, Math.min(window.innerHeight - 200, startHeight + startY - e.clientY))
    }
    const up = () => {
      target.removeEventListener('pointermove', move)
      target.removeEventListener('pointerup', up)
      localStorage.setItem('bawkbit.details-height', String(Math.round(height)))
    }
    target.addEventListener('pointermove', move)
    target.addEventListener('pointerup', up)
  }
</script>

{#if torrent}
  <aside class={['details', compact && 'sheet']} style:--height="{height}px" aria-label="details for {torrent.name}">
    {#if !compact}
      <div class="grip" role="separator" aria-orientation="horizontal" aria-label="resize details" onpointerdown={startResize}></div>
    {/if}
    {#if compact}
      <div class="sheet-head">
        <span class="name strong truncate">{torrent.name}</span>
        <button type="button" class="btn ghost icon" aria-label="close" onclick={close}><X /></button>
      </div>
    {/if}
    <div class="bar">
      <div class="tabs" role="tablist">
        {#each tabs as tab (tab.id)}
          <button
            type="button"
            role="tab"
            aria-selected={app.detailsTab === tab.id}
            class={['tab', app.detailsTab === tab.id && 'active']}
            onclick={() => (app.detailsTab = tab.id)}
          >
            <tab.icon size={14} />
            <span class="tab-label">{tab.label}</span>
            {#if tab.id === 'peers' && torrent.peers}<span class="n num">{torrent.peers}</span>{/if}
            {#if tab.id === 'files' && shown && shown.files.length > 1}<span class="n num">{shown.files.length}</span>{/if}
          </button>
        {/each}
      </div>
      <div class="actions">
        {#if isRunning(torrent)}
          <button type="button" class="btn ghost icon" aria-label="pause" title="pause" onclick={() => app.pause([torrent.id])}><Pause /></button>
        {:else}
          <button type="button" class="btn ghost icon" aria-label="resume" title="resume" onclick={() => app.resume([torrent.id])}><Play /></button>
        {/if}
        <button type="button" class="btn ghost icon" aria-label="remove" title="remove" onclick={() => app.askRemove([torrent.id])}><Trash /></button>
        {#if !compact}
          <button type="button" class="btn ghost icon" aria-label="close details" title="close" onclick={close}><X /></button>
        {/if}
      </div>
    </div>

    <div class="body" role="tabpanel">
      {#if error && !shown}
        <p class="muted pad">{error}</p>
      {:else if app.detailsTab === 'overview'}
        <div class="overview pad">
          {#if shown && shown.piece_map.length}
            <PieceMap map={shown.piece_map} label="{percent(torrent.progress)} of the pieces are here" />
          {/if}
          <dl class="grid">
            <div><dt>state</dt><dd style:color={looks[torrent.state].color}>{looks[torrent.state].label}{#if torrent.forced}<span class="muted"> · forced</span>{/if}</dd></div>
            <div><dt>progress</dt><dd class="num">{percent(torrent.progress)} <span class="muted">· {bytes(torrent.done)} of {bytes(torrent.size)}</span></dd></div>
            <div><dt>speed</dt><dd class="num"><span class="d">↓ {speed(torrent.down) || '0 B/s'}</span> <span class="u">↑ {speed(torrent.up) || '0 B/s'}</span></dd></div>
            <div><dt>time left</dt><dd class="num">{eta(torrent.eta) || '–'}</dd></div>
            {#if torrent.down_limit || torrent.up_limit}
              <div><dt>limits</dt><dd class="num"><span class="d">↓ {speed(torrent.down_limit) || 'none'}</span> <span class="u">↑ {speed(torrent.up_limit) || 'none'}</span></dd></div>
            {/if}
            {#if torrent.no_upload}<div><dt>upload</dt><dd>off</dd></div>{/if}
            <div><dt>uploaded</dt><dd class="num">{bytes(torrent.uploaded)} <span class="muted">· ratio {ratio(torrent.ratio)}</span></dd></div>
            <div><dt>peers</dt><dd class="num">{torrent.peers} <span class="muted">· {torrent.seeds} seeding</span></dd></div>
            <div><dt>added</dt><dd>{date(torrent.added)}</dd></div>
            {#if torrent.completed}<div><dt>finished</dt><dd>{date(torrent.completed)}</dd></div>{/if}
            {#if shown && shown.pieces}<div><dt>pieces</dt><dd class="num">{count(shown.pieces)} × {bytes(shown.piece_size)}</dd></div>{/if}
            {#if torrent.category}<div><dt>category</dt><dd>{torrent.category}</dd></div>{/if}
            {#if torrent.tags.length}<div><dt>tags</dt><dd>{torrent.tags.join(', ')}</dd></div>{/if}
            <div class="wide">
              <dt>folder</dt>
              <dd class="copyable">
                <span class="selectable truncate" title={shown?.save_path ?? torrent.save_path}>{shown?.save_path ?? torrent.save_path}</span>
                <button type="button" class="btn ghost small icon" aria-label="copy folder" title="copy" onclick={() => app.copy(shown?.save_path ?? torrent.save_path, 'folder')}><Copy /></button>
              </dd>
            </div>
            <div class="wide">
              <dt>hash</dt>
              <dd class="copyable">
                <span class="selectable truncate num">{torrent.id}</span>
                {#if shown}
                  <button type="button" class="btn ghost small" title="copy magnet link" onclick={() => shown && app.copy(shown.magnet, 'magnet link')}><Copy />magnet</button>
                {/if}
              </dd>
            </div>
            {#if torrent.error}<div class="wide"><dt>error</dt><dd class="err selectable">{torrent.error}</dd></div>{/if}
          </dl>
        </div>
      {:else if app.detailsTab === 'files'}
        {#if shown}
          <div class="table files">
            {#if shown.files.length > 1}
            <div class="thead">
              <button
                type="button"
                class="check"
                role="checkbox"
                aria-checked={included.size === shown.files.length ? 'true' : included.size === 0 ? 'false' : 'mixed'}
                aria-label="download every file"
                onclick={toggleAll}
              >[{included.size === shown.files.length ? 'x' : included.size === 0 ? ' ' : '-'}]</button>
              <span>{included.size === shown.files.length ? 'every file' : `${included.size} of ${shown.files.length}`}</span>
            </div>
            {/if}
            {#each shown.files as file (file.index)}
              {@const [dir, name] = splitPath(file.path)}
              {@const on = included.has(file.index)}
              <div class={['tr', !on && 'off']}>
                <button type="button" class="check" role="checkbox" aria-checked={on} aria-label="download {name}" onclick={() => toggleFile(file.index)}>[{on ? 'x' : ' '}]</button>
                <span class="truncate path" title={file.path}><span class="muted">{dir}</span><span class="strong">{name}</span></span>
                <span class="r num muted">{bytes(file.size)}</span>
                <span class="fprog num">
                  <span class="mini"><span style:width="{(file.done / Math.max(1, file.size)) * 100}%"></span></span>
                  <span class="muted">{percent(file.done / Math.max(1, file.size))}</span>
                </span>
              </div>
            {/each}
            {#if ownPending && ownPending.size === 0}<p class="err pad">pick at least one file.</p>{/if}
          </div>
        {/if}
      {:else if app.detailsTab === 'peers'}
        {#if shown}
          {#if shown.peers.length === 0}
            <p class="muted pad">{isRunning(torrent) ? 'no peers connected yet.' : 'resume to connect to peers.'}</p>
          {:else}
            <div class="table peers">
              {#each shown.peers as peer (peer.addr)}
                <div class="tr">
                  <span class="truncate num selectable">
                    {#if peer.country}
                      <img class="flag" src="/flags/{peer.country}.svg" alt={peer.country} title={countryName(peer.country)} width="16" height="11" onerror={(e) => ((e.currentTarget as HTMLImageElement).hidden = true)} />
                    {/if}{peer.addr}
                  </span>
                  <span class="truncate muted">{peer.client ?? ''}</span>
                  <span class="r num d">{speed(peer.down)}</span>
                  <span class="r num u">{speed(peer.up)}</span>
                  <span class="r num muted" title="downloaded from this peer">{peer.downloaded ? bytes(peer.downloaded) : ''}</span>
                  <span class="r muted">{peer.transport ?? ''}</span>
                </div>
              {/each}
            </div>
          {/if}
        {/if}
      {:else if shown}
        {#if shown.trackers.length === 0}
          <p class="muted pad">no trackers. peers come from DHT.</p>
        {:else}
          <div class="table trackers">
            {#each shown.trackers as url (url)}
              {@const report = shown.tracker_reports?.find((r) => r.url === url)}
              {@const said = trackerSaid(report)}
              <div class="tr">
                <span class={['dot', report?.status ?? 'waiting']} title={said.label}></span>
                <span class="tracker">
                  <span class="truncate selectable">{url}</span>
                  <span class={['truncate', 'said', report?.status ?? 'waiting']} title={said.text}>{said.label} · {said.text}</span>
                </span>
                <button type="button" class="btn ghost small icon" aria-label="copy tracker" title="copy" onclick={() => app.copy(url, 'tracker')}><Copy /></button>
                <button type="button" class="btn ghost small icon" aria-label="remove tracker {url}" title="remove" disabled={trackerBusy} onclick={() => removeTracker(url)}><X /></button>
              </div>
            {/each}
          </div>
        {/if}
        <form class="add-trackers" onsubmit={addTrackers}>
          <textarea
            class="input"
            rows="2"
            placeholder="add trackers, one per line"
            aria-label="add trackers, one per line"
            spellcheck="false"
            bind:value={trackerText}
            onkeydown={trackerKeys}
          ></textarea>
          <button type="submit" class="btn small" disabled={trackerBusy || !trackerText.trim()}>add</button>
        </form>
        <p class="muted pad list-trackers">
          {#if shown.private}
            private torrent: public trackers stay off it.
          {:else}
            <button type="button" class="btn small" disabled={trackerBusy} onclick={addListTrackers}>add public trackers</button>
            <span>from the lists under settings, trackers.</span>
          {/if}
        </p>
      {/if}
    </div>
  </aside>
{/if}

<style>
  .details {
    position: relative;
    display: flex;
    flex-direction: column;
    height: var(--height);
    flex: none;
    border-top: 1px solid var(--border-weak);
    background: var(--bg);
  }
  .grip {
    position: absolute;
    top: -4px;
    left: 0;
    right: 0;
    height: 8px;
    cursor: row-resize;
    z-index: 2;
    touch-action: none;
  }
  .grip:hover {
    background: linear-gradient(transparent 3px, var(--border) 3px, var(--border) 4px, transparent 4px);
  }
  .bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    height: 38px;
    padding: 0 6px 0 8px;
    border-bottom: 1px solid var(--border-weak);
    flex: none;
  }
  .tabs {
    display: flex;
    gap: 2px;
    min-width: 0;
    overflow-x: auto;
  }
  .tab {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    height: 28px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--radius);
    background: none;
    color: var(--text-weak);
    white-space: nowrap;
    cursor: pointer;
  }
  .tab:hover {
    color: var(--text-strong);
  }
  .tab.active {
    background: var(--bg-weak-hover);
    color: var(--text-strong);
  }
  .tab .n {
    color: var(--text-weaker);
    font-size: 12px;
  }
  .actions {
    display: flex;
    gap: 2px;
    flex: none;
  }
  .body {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }
  .pad {
    padding: 14px 16px;
  }
  .overview {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
    gap: 10px 24px;
    margin: 0;
  }
  .grid > div {
    min-width: 0;
  }
  .grid .wide {
    grid-column: 1 / -1;
  }
  dt {
    color: var(--text-weaker);
    font-size: 12px;
  }
  dd {
    margin: 0;
    color: var(--text-strong);
    min-width: 0;
  }
  .copyable {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .d {
    color: var(--yolk);
  }
  .u {
    color: var(--success);
  }
  .err {
    color: var(--danger);
  }
  .table {
    display: flex;
    flex-direction: column;
  }
  .tr,
  .thead {
    display: grid;
    align-items: center;
    gap: 12px;
    min-height: 30px;
    padding: 0 16px;
    border-bottom: 1px solid color-mix(in oklch, var(--border-weak) 55%, transparent);
    content-visibility: auto;
    contain-intrinsic-size: auto 30px;
  }
  .files .tr,
  .files .thead {
    grid-template-columns: 30px minmax(0, 1fr) 80px 110px;
  }
  .thead {
    color: var(--text-weaker);
    font-size: 12px;
  }
  .tr.off .path,
  .tr.off .path .strong {
    color: var(--text-weaker);
  }
  .check {
    padding: 0;
    border: 0;
    background: none;
    color: var(--text-strong);
    white-space: pre;
    cursor: pointer;
    text-align: left;
  }
  .fprog {
    display: flex;
    align-items: center;
    gap: 8px;
    justify-content: flex-end;
  }
  .fprog .muted {
    min-width: 4.5ch;
    text-align: right;
  }
  .mini {
    width: 50px;
    height: 3px;
    border-radius: 3px;
    background: var(--border-weak);
    overflow: hidden;
  }
  .mini span {
    display: block;
    height: 100%;
    background: var(--yolk);
  }
  .r {
    text-align: right;
    white-space: nowrap;
  }
  .peers .tr {
    grid-template-columns: minmax(150px, 1.1fr) minmax(0, 1.3fr) 84px 84px 76px 44px;
  }
  .flag {
    display: inline-block;
    margin-right: 8px;
    vertical-align: -1px;
    border-radius: 2px;
  }
  .trackers .tr {
    grid-template-columns: 8px minmax(0, 1fr) 24px 24px;
    gap: 6px 10px;
    padding-block: 6px;
  }
  .tracker {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .said {
    color: var(--text-weaker);
    font-size: 12px;
  }
  .said.working {
    color: var(--success);
  }
  .said.failing {
    color: var(--danger);
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--text-weaker);
  }
  .dot.working {
    background: var(--success);
  }
  .dot.failing {
    background: var(--danger);
  }
  .list-trackers {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    padding-top: 0;
  }
  .add-trackers {
    display: flex;
    align-items: flex-end;
    gap: 8px;
    padding: 12px 16px 16px;
  }
  .add-trackers textarea {
    flex: 1;
    min-height: 54px;
  }

  .sheet {
    position: fixed;
    inset: 0;
    z-index: 80;
    height: auto;
    border-top: 0;
  }
  .sheet-head {
    display: flex;
    align-items: center;
    gap: 8px;
    height: var(--header);
    padding: 0 4px 0 14px;
    border-bottom: 1px solid var(--border-weak);
    flex: none;
  }
  .sheet-head .name {
    flex: 1;
  }
  .sheet .tab-label {
    display: none;
  }
  .sheet .tab.active .tab-label {
    display: inline;
  }
  .sheet .grid {
    grid-template-columns: 1fr 1fr;
  }
  .sheet .files .tr,
  .sheet .files .thead {
    grid-template-columns: 30px minmax(0, 1fr) 64px;
  }
  .sheet .fprog {
    display: none;
  }
  .sheet .peers .tr {
    grid-template-columns: minmax(0, 1fr) 76px 76px;
  }
  .sheet .peers .tr > :nth-child(2),
  .sheet .peers .tr > :nth-child(5),
  .sheet .peers .tr > :nth-child(6) {
    display: none;
  }
</style>
