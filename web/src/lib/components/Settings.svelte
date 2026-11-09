<script lang="ts">
  import Archive from '@lucide/svelte/icons/archive'
  import Download from '@lucide/svelte/icons/download'
  import Gauge from '@lucide/svelte/icons/gauge'
  import Globe from '@lucide/svelte/icons/globe'
  import ListOrdered from '@lucide/svelte/icons/list-ordered'
  import LogOut from '@lucide/svelte/icons/log-out'
  import Network from '@lucide/svelte/icons/network'
  import Palette from '@lucide/svelte/icons/palette'
  import Plus from '@lucide/svelte/icons/plus'
  import RssIcon from '@lucide/svelte/icons/rss'
  import Search from '@lucide/svelte/icons/search'
  import Server from '@lucide/svelte/icons/server'
  import Trash from '@lucide/svelte/icons/trash-2'
  import Upload from '@lucide/svelte/icons/upload'
  import type { Component } from 'svelte'
  import { api, type Indexer, type IndexerKind, type Settings, type TrackerLists } from '$lib/api'
  import { limitBytes, limitUnit, limitValue } from '$lib/format'
  import { app, type SettingsTab, type ThemeChoice } from '$lib/state.svelte'
  import Checkbox from './Checkbox.svelte'

  type LimitKey = 'down_limit' | 'up_limit' | 'alt_down_limit' | 'alt_up_limit'

  const tabs: { id: SettingsTab; label: string; icon: Component<{ size?: number }> }[] = [
    { id: 'downloads', label: 'downloads', icon: Download },
    { id: 'connection', label: 'connection', icon: Network },
    { id: 'trackers', label: 'trackers', icon: Server },
    { id: 'speed', label: 'speed', icon: Gauge },
    { id: 'queue', label: 'queue', icon: ListOrdered },
    { id: 'search', label: 'search', icon: Search },
    { id: 'rss', label: 'rss', icon: RssIcon },
    { id: 'web', label: 'web UI', icon: Globe },
    { id: 'backup', label: 'backup', icon: Archive },
    { id: 'appearance', label: 'appearance', icon: Palette }
  ]
  const placeholders: [string, string][] = [
    ['%N', 'name'],
    ['%F', 'content path'],
    ['%D', 'folder'],
    ['%L', 'category'],
    ['%G', 'tags'],
    ['%Z', 'size'],
    ['%C', 'file count'],
    ['%I', 'id']
  ]
  const themes: { id: ThemeChoice; label: string }[] = [
    { id: 'system', label: 'system' },
    { id: 'dark', label: 'dark' },
    { id: 'light', label: 'light' }
  ]
  const days = ['m', 't', 'w', 't', 'f', 's', 's']
  const dayNames = ['monday', 'tuesday', 'wednesday', 'thursday', 'friday', 'saturday', 'sunday']

  let draft = $state<Settings | null>(null)
  let units = $state<Record<LimitKey, 'k' | 'm'>>({ down_limit: 'm', up_limit: 'm', alt_down_limit: 'm', alt_up_limit: 'k' })
  let status = $state<{ text: string; error: boolean }>({ text: '', error: false })
  let password = $state('')
  let queue: Promise<void> = Promise.resolve()
  let statusTimer: ReturnType<typeof setTimeout> | null = null

  const altSpeed = $derived(app.snapshot?.totals.alt_speed ?? draft?.alt_speed ?? false)

  api
    .settings()
    .then((settings) => {
      draft = settings
      units = {
        down_limit: limitUnit(settings.down_limit),
        up_limit: limitUnit(settings.up_limit),
        alt_down_limit: limitUnit(settings.alt_down_limit),
        alt_up_limit: limitUnit(settings.alt_up_limit)
      }
    })
    .catch((err: Error) => (status = { text: err.message, error: true }))

  function show(text: string, error: boolean): void {
    status = { text, error }
    if (statusTimer) clearTimeout(statusTimer)
    if (!error) statusTimer = setTimeout(() => (status = { text: '', error: false }), 2000)
  }

  function save(change: (s: Settings) => void, newPassword?: string): void {
    if (!draft) return
    change(draft)
    const body: Settings = { ...$state.snapshot(draft), alt_speed: altSpeed }
    if (newPassword) body.web = { ...body.web, password: newPassword }
    queue = queue.then(async () => {
      try {
        const saved = await api.saveSettings(body)
        draft = saved
        app.defaults = saved
        show(newPassword ? 'password changed. log in again.' : 'saved', false)
      } catch (err) {
        show((err as Error).message, true)
      }
    })
  }

  function int(value: string, min: number, max: number): number | null {
    const n = Number(value)
    if (!value.trim()) return 0
    if (!Number.isInteger(n) || n < min || n > max) return null
    return n
  }

  function setNumber(key: 'listen_port' | 'peer_limit' | 'max_active_downloads' | 'seed_minutes' | 'rss_minutes', value: string, min: number, max: number, label: string): void {
    const n = int(value, key === 'listen_port' ? 1 : 0, max)
    if (n === null || (key === 'listen_port' && n === 0)) {
      show(`${label}: use a whole number from ${min} to ${max}.`, true)
      return
    }
    save((s) => (s[key] = n))
  }

  function setRatio(value: string): void {
    const n = value.trim() ? Number(value) : 0
    if (!Number.isFinite(n) || n < 0) {
      show('ratio: use a number like 2 or 1.5.', true)
      return
    }
    save((s) => (s.ratio_limit = n))
  }

  function setLimit(key: LimitKey, value: string): void {
    save((s) => (s[key] = limitBytes(value, units[key])))
  }

  function setUnit(key: LimitKey, unit: 'k' | 'm'): void {
    if (!draft || units[key] === unit) return
    const shown = limitValue(draft[key], units[key])
    units[key] = unit
    if (shown) setLimit(key, shown)
  }

  function time(minutes: number): string {
    return `${String(Math.floor(minutes / 60)).padStart(2, '0')}:${String(minutes % 60).padStart(2, '0')}`
  }

  function minutes(value: string): number | null {
    const match = /^(\d{1,2}):(\d{2})$/.exec(value)
    if (!match) return null
    return Number(match[1]) * 60 + Number(match[2])
  }

  function setTime(key: 'from' | 'to', value: string): void {
    const n = minutes(value)
    if (n !== null) save((s) => (s.schedule[key] = n))
  }

  function risky(message: string, confirm: string, change: (s: Settings) => void): void {
    app.confirm = {
      message,
      confirm,
      danger: true,
      run: () => save(change)
    }
  }

  let lists = $state<TrackerLists | null>(null)
  let readingLists = $state(false)

  api
    .trackerLists()
    .then((info) => (lists = info))
    .catch(() => {})

  function setLists(text: string): void {
    const typed = text
      .split('\n')
      .map((line) => line.trim())
      .filter(Boolean)
    save((s) => (s.trackers.lists = typed))
  }

  async function readLists(): Promise<void> {
    readingLists = true
    await queue
    try {
      lists = await api.readTrackerLists()
      show(lists.errors.length ? 'some lists did not answer' : `${lists.trackers} trackers`, lists.errors.length > 0)
    } catch (err) {
      show((err as Error).message, true)
    } finally {
      readingLists = false
    }
  }

  function addListsEverywhere(): void {
    const ids = app.torrents.map((t) => t.id)
    app.confirm = {
      message: 'add public trackers to every torrent? private torrents are skipped. each other torrent drops its peers once and finds them again.',
      confirm: 'add them',
      run: async () => {
        if (await app.act({ action: 'add_list_trackers', ids })) show('public trackers added', false)
      }
    }
  }

  function ago(unix: number): string {
    const minutes = Math.max(1, Math.round((Date.now() / 1000 - unix) / 60))
    if (minutes < 60) return `${minutes}m ago`
    if (minutes < 60 * 48) return `${Math.round(minutes / 60)}h ago`
    return `${Math.round(minutes / 1440)}d ago`
  }

  const DEFAULT_URL: Record<IndexerKind, string> ={ prowlarr: 'http://localhost:9696', jackett: 'http://localhost:9117' }
  let tests = $state<Record<string, { text: string; error: boolean; busy: boolean }>>({})

  function addIndexer(kind: IndexerKind): void {
    const indexer: Indexer = { id: '', kind, name: kind, url: DEFAULT_URL[kind], api_key: '', has_key: false, enabled: true }
    save((s) => s.indexers.push(indexer))
  }

  function editIndexer(ix: number, change: (indexer: Indexer) => void): void {
    save((s) => {
      const indexer = s.indexers[ix]
      if (indexer) change(indexer)
    })
  }

  async function testIndexer(ix: number): Promise<void> {
    if (!draft) return
    await queue
    const indexer = draft.indexers[ix]
    if (!indexer) return
    const key = indexer.id || String(ix)
    tests[key] = { text: 'testing…', error: false, busy: true }
    try {
      const { message } = await api.testIndexer($state.snapshot(indexer))
      tests[key] = { text: message, error: false, busy: false }
    } catch (err) {
      tests[key] = { text: (err as Error).message, error: true, busy: false }
    }
  }

  let backupInput = $state<HTMLInputElement>()

  async function exportBackup(): Promise<void> {
    try {
      const backup = await api.backup()
      const blob = new Blob([JSON.stringify(backup, null, 2)], { type: 'application/json' })
      const url = URL.createObjectURL(blob)
      const link = document.createElement('a')
      link.href = url
      link.download = 'bawkbit-backup.json'
      link.click()
      setTimeout(() => URL.revokeObjectURL(url), 1000)
    } catch (err) {
      show((err as Error).message, true)
    }
  }

  async function importBackup(file: File | undefined): Promise<void> {
    if (backupInput) backupInput.value = ''
    if (!file) return
    let backup: unknown
    try {
      backup = JSON.parse(await file.text())
    } catch {
      show('that file is not a bawkbit backup.', true)
      return
    }
    app.confirm = {
      message: `restore ${file.name}? its settings replace these; its categories, tags, feeds and rules are added to yours.`,
      confirm: 'restore',
      run: async () => {
        try {
          await api.restore(backup)
          draft = await api.settings()
          app.defaults = draft
          void app.loadRss()
          show('restored', false)
        } catch (err) {
          show((err as Error).message, true)
        }
      }
    }
  }

  function changePassword(event: SubmitEvent): void {
    event.preventDefault()
    if (password.length < 8) {
      show('use at least 8 characters for the password.', true)
      return
    }
    const next = password
    password = ''
    save(() => {}, next)
  }
</script>

<div class="page">
  <nav class="tabs" aria-label="settings">
    {#each tabs as t (t.id)}
      <button type="button" class={['tab', app.settingsTab === t.id && 'active']} aria-current={app.settingsTab === t.id} onclick={() => (app.settingsTab = t.id)}>
        <t.icon size={14} />{t.label}
      </button>
    {/each}
    <div class="foot">
      <button type="button" class="tab" onclick={() => app.logout()}><LogOut size={14} />log out</button>
      {#if app.session}<span class="weaker version">bawkbit {app.session.version}</span>{/if}
    </div>
  </nav>

  <div class="panel">
    <p class={['status', status.error && 'error']} role={status.error ? 'alert' : 'status'}>{status.text}</p>

    {#if !draft}
      <p class="muted">{status.error ? '' : 'loading…'}</p>
    {:else if app.settingsTab === 'downloads'}
      <label class="field">
        <span class="label">folder</span>
        <input class="input" spellcheck="false" value={draft.download_dir} onchange={(e) => save((s) => (s.download_dir = e.currentTarget.value.trim()))} />
      </label>
      <div class="field">
        <label class="label" for="watch-dir">watch folder</label>
        <input
          id="watch-dir"
          class="input"
          spellcheck="false"
          placeholder="off"
          aria-describedby="watch-hint"
          value={draft.watch_dir}
          onchange={(e) => save((s) => (s.watch_dir = e.currentTarget.value.trim()))}
        />
        <span class="hint" id="watch-hint">.torrent files saved here are added on their own.</span>
      </div>
      <div class="checks">
        <Checkbox checked={draft.start_paused} label="add torrents paused" onchange={(v) => save((s) => (s.start_paused = v))} />
      </div>
      <h3>when a download finishes</h3>
      <div class="field">
        <label class="label" for="on-finish">run</label>
        <input id="on-finish" class="input" readonly spellcheck="false" placeholder="nothing" aria-describedby="on-finish-hint" value={draft.on_finish} />
        <span class="hint" id="on-finish-hint">set in the bawkbit desktop app or in settings.json, since it runs a program on that computer.</span>
        <span class="hint placeholders">
          {#each placeholders as [code, meaning] (code)}<span><code>{code}</code> {meaning}</span>{/each}
        </span>
      </div>
    {:else if app.settingsTab === 'connection'}
      <label class="row">
        <span>listening port</span>
        <input class="input narrow num" type="number" min="1" max="65535" value={draft.listen_port} onchange={(e) => setNumber('listen_port', e.currentTarget.value, 1, 65535, 'port')} />
      </label>
      <label class="row">
        <span>peers per torrent</span>
        <input
          class="input narrow num"
          type="number"
          min="0"
          max="10000"
          placeholder="auto"
          value={draft.peer_limit || ''}
          onchange={(e) => setNumber('peer_limit', e.currentTarget.value, 0, 10000, 'peers')}
        />
      </label>
      <div class="checks">
        <Checkbox checked={draft.upnp} label="open the port on the router (UPnP)" onchange={(v) => save((s) => (s.upnp = v))} />
        <Checkbox checked={draft.dht} label="find peers without trackers (DHT)" onchange={(v) => save((s) => (s.dht = v))} />
        <Checkbox checked={draft.utp} label="uTP" onchange={(v) => save((s) => (s.utp = v))} />
        <span class="hint indent">also connect to peers over uTP. the engine still calls it new.</span>
        <Checkbox checked={draft.peer_countries} label="show peer countries" onchange={(v) => save((s) => (s.peer_countries = v))} />
        <span class="hint indent"
          >a flag for each peer, from <a href="https://db-ip.com" target="_blank" rel="noreferrer">IP geolocation by DB-IP</a>. the database downloads once a month.</span
        >
      </div>
      {#if app.snapshot && !app.snapshot.totals.port}
        <p class="hint warn">bawkbit is not listening for incoming peers. another program may be using the port.</p>
      {/if}
    {:else if app.settingsTab === 'trackers'}
      <div class="checks">
        <Checkbox
          checked={draft.trackers.add_from_lists}
          label="add public trackers to new torrents"
          onchange={(v) => save((s) => (s.trackers.add_from_lists = v))}
        />
        <span class="hint indent">more trackers find more peers. private torrents never get them.</span>
      </div>
      <div class="field">
        <label class="label" for="tracker-lists">tracker lists</label>
        <textarea
          id="tracker-lists"
          class="input lists"
          rows="3"
          spellcheck="false"
          placeholder="list addresses, one per line"
          aria-describedby="lists-hint"
          value={draft.trackers.lists.join('\n')}
          onchange={(e) => setLists(e.currentTarget.value)}
        ></textarea>
        <span class="hint" id="lists-hint">text files with one tracker on each line. repeats are left out.</span>
        <div class="inline wrap">
          <button type="button" class="btn small" disabled={readingLists} onclick={readLists}>{readingLists ? 'reading…' : 'read them now'}</button>
          <button type="button" class="btn small" disabled={!app.torrents.length} onclick={addListsEverywhere}>add to every torrent now</button>
          {#if lists}
            <span class="hint">{lists.fetched ? `${lists.trackers} trackers, read ${ago(lists.fetched)}` : 'not read yet'}</span>
          {/if}
        </div>
        {#each lists?.errors ?? [] as error (error)}<span class="hint error">{error}</span>{/each}
      </div>
      <div class="checks">
        <Checkbox checked={draft.trackers.remove_dead} label="remove dead trackers" onchange={(v) => save((s) => (s.trackers.remove_dead = v))} />
        <span class="hint indent">takes a tracker off a torrent after an hour of failing without one answer. never on private torrents.</span>
      </div>
    {:else if app.settingsTab === 'speed'}
      {@render limit('down_limit', 'download')}
      {@render limit('up_limit', 'upload')}
      <h3>slow mode</h3>
      {@render limit('alt_down_limit', 'download')}
      {@render limit('alt_up_limit', 'upload')}
      <div class="checks">
        <Checkbox checked={altSpeed} label="on now" onchange={(v) => app.setAltSpeed(v)} />
        <Checkbox checked={draft.schedule.enabled} label="on a schedule" onchange={(v) => save((s) => (s.schedule.enabled = v))} />
      </div>
      {#if draft.schedule.enabled}
        <div class="schedule">
          <input class="input time" type="time" aria-label="from" value={time(draft.schedule.from)} onchange={(e) => setTime('from', e.currentTarget.value)} />
          <span class="muted">to</span>
          <input class="input time" type="time" aria-label="to" value={time(draft.schedule.to)} onchange={(e) => setTime('to', e.currentTarget.value)} />
          <div class="days" role="group" aria-label="days">
            {#each days as day, ix (ix)}
              {@const on = (draft.schedule.days & (1 << ix)) !== 0}
              <button
                type="button"
                class={['day', on && 'on']}
                aria-pressed={on}
                aria-label={dayNames[ix]}
                title={dayNames[ix]}
                onclick={() => save((s) => (s.schedule.days ^= 1 << ix))}>{day}</button
              >
            {/each}
          </div>
        </div>
      {/if}
    {:else if app.settingsTab === 'queue'}
      <label class="row">
        <span>downloads at once</span>
        <input
          class="input narrow num"
          type="number"
          min="0"
          max="1000"
          placeholder="no limit"
          value={draft.max_active_downloads || ''}
          onchange={(e) => setNumber('max_active_downloads', e.currentTarget.value, 0, 1000, 'downloads at once')}
        />
      </label>
      <h3>stop seeding</h3>
      <label class="row">
        <span>at ratio</span>
        <input class="input narrow num" type="number" min="0" step="0.1" placeholder="never" value={draft.ratio_limit || ''} onchange={(e) => setRatio(e.currentTarget.value)} />
      </label>
      <label class="row">
        <span>after minutes</span>
        <input
          class="input narrow num"
          type="number"
          min="0"
          max="5256000"
          placeholder="never"
          value={draft.seed_minutes || ''}
          onchange={(e) => setNumber('seed_minutes', e.currentTarget.value, 0, 5256000, 'minutes')}
        />
      </label>
    {:else if app.settingsTab === 'search'}
      {#each draft.indexers as indexer, ix (indexer.id || ix)}
        {@const test = tests[indexer.id || String(ix)]}
        <div class="indexer">
          <div class="indexer-head">
            <span class="kind">{indexer.kind}</span>
            <input
              class="input name-input"
              spellcheck="false"
              aria-label="name"
              value={indexer.name}
              onchange={(e) => editIndexer(ix, (i) => (i.name = e.currentTarget.value.trim()))}
            />
            <Checkbox checked={indexer.enabled} label="on" onchange={(v) => editIndexer(ix, (i) => (i.enabled = v))} />
            <button
              type="button"
              class="btn ghost icon"
              aria-label="remove {indexer.name || indexer.kind}"
              title="remove"
              onclick={() =>
                risky(`remove ${indexer.name || indexer.kind}? search stops asking it. its address and key are forgotten.`, 'remove', (s) =>
                  s.indexers.splice(ix, 1)
                )}
            >
              <Trash />
            </button>
          </div>
          <input
            class="input"
            spellcheck="false"
            aria-label="{indexer.kind} address"
            placeholder={DEFAULT_URL[indexer.kind]}
            value={indexer.url}
            onchange={(e) => editIndexer(ix, (i) => (i.url = e.currentTarget.value.trim()))}
          />
          <div class="inline">
            <input
              class="input"
              type="password"
              autocomplete="off"
              spellcheck="false"
              aria-label="{indexer.kind} API key"
              placeholder={indexer.has_key ? 'key saved · type to replace' : 'api key'}
              value={indexer.api_key}
              onchange={(e) => {
                const key = e.currentTarget.value.trim()
                if (key) editIndexer(ix, (i) => (i.api_key = key))
              }}
            />
            <button type="button" class="btn" disabled={test?.busy} onclick={() => testIndexer(ix)}>test</button>
          </div>
          {#if test}
            <p class={['test', test.error && 'error']} role={test.error ? 'alert' : 'status'}>{test.text}</p>
          {/if}
        </div>
      {/each}
      <div class="inline adds">
        <button type="button" class="btn" onclick={() => addIndexer('prowlarr')}><Plus />add prowlarr</button>
        <button type="button" class="btn" onclick={() => addIndexer('jackett')}><Plus />add jackett</button>
      </div>
      <p class="hint">the API key is in prowlarr under settings → general, and at the top right of jackett's dashboard.</p>
    {:else if app.settingsTab === 'rss'}
      <label class="row">
        <span>check feeds every</span>
        <span class="inline">
          <input
            class="input narrow num"
            type="number"
            min="0"
            max="10080"
            placeholder="by hand"
            value={draft.rss_minutes || ''}
            onchange={(e) => setNumber('rss_minutes', e.currentTarget.value, 0, 10080, 'minutes')}
          />
          <span class="muted">min</span>
        </span>
      </label>
      <p class="hint">empty checks only when you press refresh. bawkbit waits at least 5 minutes between checks of one feed.</p>
      <div class="inline adds">
        <button type="button" class="btn" onclick={() => app.openRss()}><RssIcon />feeds and rules</button>
      </div>
    {:else if app.settingsTab === 'backup'}
      <p class="hint">settings, categories, tags, feeds and rules, to move to another computer. passwords and API keys are left out.</p>
      <div class="inline adds">
        <button type="button" class="btn" onclick={exportBackup}><Download />export</button>
        <button type="button" class="btn" onclick={() => backupInput?.click()}><Upload />import</button>
      </div>
      <input bind:this={backupInput} type="file" accept=".json,application/json" hidden onchange={(e) => void importBackup(e.currentTarget.files?.[0])} />
    {:else if app.settingsTab === 'web'}
      <div class="checks">
        <Checkbox checked={draft.web.enabled} label="web UI on" onchange={(v) => (v ? save((s) => (s.web.enabled = v)) : risky('turn off the web UI? this page stops working until it is turned on in the bawkbit app.', 'turn off', (s) => (s.web.enabled = false)))} />
      </div>
      <label class="row">
        <span>address</span>
        <input class="input wide" spellcheck="false" value={draft.web.address}
          onchange={(e) => {
            const value = e.currentTarget.value.trim()
            risky(`move the web UI to ${value}? this page stops working here.`, 'move', (s) => (s.web.address = value))
          }}
        />
      </label>
      <label class="row">
        <span>port</span>
        <input
          class="input narrow num"
          type="number"
          min="1"
          max="65535"
          value={draft.web.port}
          onchange={(e) => {
            const n = int(e.currentTarget.value, 1, 65535)
            if (!n) show('port: use a whole number from 1 to 65535.', true)
            else risky(`move the web UI to port ${n}? open it at that port afterwards.`, 'move', (s) => (s.web.port = n))
          }}
        />
      </label>
      <label class="row">
        <span>name</span>
        <input class="input wide" spellcheck="false" autocomplete="username" value={draft.web.username} onchange={(e) => save((s) => (s.web.username = e.currentTarget.value.trim()))} />
      </label>
      <form class="row" onsubmit={changePassword}>
        <label for="new-password">new password</label>
        <span class="inline">
          <input id="new-password" class="input wide" type="password" autocomplete="new-password" bind:value={password} />
          <button type="submit" class="btn" disabled={!password}>set</button>
        </span>
      </form>
      <div class="checks">
        <Checkbox checked={draft.web.local_bypass} label="no password on this computer" onchange={(v) => save((s) => (s.web.local_bypass = v))} />
      </div>
    {:else if app.settingsTab === 'appearance'}
      <div class="row">
        <span>theme</span>
        <div class="seg">
          {#each themes as t (t.id)}
            <button type="button" class={{ active: app.theme === t.id }} aria-pressed={app.theme === t.id} onclick={() => app.setTheme(t.id)}>{t.label}</button>
          {/each}
        </div>
      </div>
    {/if}
  </div>
</div>

{#snippet limit(key: LimitKey, label: string)}
  {#if draft}
    <div class="row">
      <label for="limit-{key}">{label}</label>
      <span class="inline">
        <input
          id="limit-{key}"
          class="input narrow num"
          type="number"
          min="0"
          step="any"
          placeholder="no limit"
          value={limitValue(draft[key], units[key])}
          onchange={(e) => setLimit(key, e.currentTarget.value)}
        />
        <span class="seg" role="group" aria-label="{label} unit">
          <button type="button" class={{ active: units[key] === 'k' }} aria-pressed={units[key] === 'k'} onclick={() => setUnit(key, 'k')}>KB/s</button>
          <button type="button" class={{ active: units[key] === 'm' }} aria-pressed={units[key] === 'm'} onclick={() => setUnit(key, 'm')}>MB/s</button>
        </span>
      </span>
    </div>
  {/if}
{/snippet}

<style>
  .page {
    container-type: inline-size;
    display: grid;
    grid-template-columns: 180px minmax(0, 560px);
    gap: 40px;
    align-items: start;
    align-content: start;
    height: 100%;
    padding: 28px 32px 64px;
    overflow-y: auto;
  }
  .tabs {
    position: sticky;
    top: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .tab {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 32px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--radius);
    background: none;
    color: var(--text-weak);
    text-align: left;
    white-space: nowrap;
    cursor: pointer;
  }
  .tab:hover {
    color: var(--text-strong);
    background: var(--bg-weak);
  }
  .tab.active {
    background: var(--bg-weak-hover);
    color: var(--text-strong);
  }
  .foot {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-top: 16px;
    padding-top: 12px;
    border-top: 1px solid var(--border-weak);
  }
  .version {
    padding: 6px 10px;
    font-size: 12px;
  }
  .panel {
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-width: 0;
  }
  .status {
    min-height: 20px;
    margin-bottom: 2px;
    color: var(--success);
    font-size: 12px;
  }
  .status.error {
    color: var(--danger);
  }
  h3 {
    margin: 18px 0 0;
    color: var(--text-weak);
    font-size: 12px;
    font-weight: 600;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .label {
    color: var(--text-weak);
    font-size: 12px;
  }
  .hint {
    color: var(--text-weak);
    font-size: 12px;
  }
  .hint.warn {
    color: var(--warning);
  }
  .hint.error {
    color: var(--danger);
    overflow-wrap: anywhere;
  }
  .lists {
    min-height: 64px;
    resize: vertical;
  }
  .inline.wrap {
    flex-wrap: wrap;
  }
  .hint.indent {
    margin: -4px 0 0 28px;
  }
  .placeholders {
    display: flex;
    flex-wrap: wrap;
    gap: 2px 14px;
  }
  code {
    color: var(--text);
    font-family: inherit;
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    min-height: 40px;
  }
  .inline {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .checks {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin: 6px 0;
  }
  .narrow {
    width: 110px;
  }
  .wide {
    width: min(280px, 55cqw);
  }
  .time {
    width: 110px;
  }
  .seg {
    display: flex;
    border: 1px solid var(--border-weak);
    border-radius: var(--radius);
    overflow: hidden;
  }
  .seg button {
    height: 30px;
    padding: 0 10px;
    border: 0;
    border-left: 1px solid var(--border-weak);
    background: none;
    color: var(--text-weak);
    cursor: pointer;
  }
  .seg button:first-child {
    border-left: 0;
  }
  .seg button:hover {
    color: var(--text-strong);
  }
  .seg button.active {
    background: var(--bg-strong);
    color: var(--text-inverted);
  }
  .schedule {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px;
    padding-left: 28px;
  }
  .days {
    display: flex;
    gap: 4px;
  }
  .day {
    width: 28px;
    height: 28px;
    border: 1px solid var(--border-weak);
    border-radius: var(--radius);
    background: none;
    color: var(--text-weak);
    cursor: pointer;
  }
  .day.on {
    border-color: var(--yolk);
    background: var(--yolk-dim);
    color: var(--text-strong);
  }
  .indexer {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px;
    border: 1px solid var(--border-weak);
    border-radius: var(--radius-lg);
  }
  .indexer-head {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .kind {
    flex: none;
    padding: 1px 6px;
    border-radius: 3px;
    background: var(--bg-weak-hover);
    color: var(--text-weak);
    font-size: 12px;
  }
  .name-input {
    flex: 1;
    min-width: 0;
  }
  .indexer .inline .input {
    flex: 1;
    min-width: 0;
  }
  .test {
    color: var(--success);
    font-size: 12px;
  }
  .test.error {
    color: var(--danger);
  }
  .adds {
    margin-top: 4px;
  }
  @media (max-width: 720px) {
    .page {
      grid-template-columns: minmax(0, 1fr);
      gap: 16px;
      padding: 12px 14px 48px;
    }
    .tabs {
      position: static;
      flex-direction: row;
      flex-wrap: wrap;
    }
    .foot {
      flex-direction: row;
      align-items: center;
      margin: 0;
      padding: 0;
      border: 0;
    }
    .version {
      display: none;
    }
  }
</style>
