<script lang="ts">
  import Download from '@lucide/svelte/icons/download'
  import Gauge from '@lucide/svelte/icons/gauge'
  import Globe from '@lucide/svelte/icons/globe'
  import ListOrdered from '@lucide/svelte/icons/list-ordered'
  import LogOut from '@lucide/svelte/icons/log-out'
  import Network from '@lucide/svelte/icons/network'
  import Palette from '@lucide/svelte/icons/palette'
  import type { Component } from 'svelte'
  import { api, type Settings } from '$lib/api'
  import { limitBytes, limitUnit, limitValue } from '$lib/format'
  import { app, type ThemeChoice } from '$lib/state.svelte'
  import Checkbox from './Checkbox.svelte'

  type Tab = 'downloads' | 'connection' | 'speed' | 'queue' | 'web' | 'appearance'
  type LimitKey = 'down_limit' | 'up_limit' | 'alt_down_limit' | 'alt_up_limit'

  const tabs: { id: Tab; label: string; icon: Component<{ size?: number }> }[] = [
    { id: 'downloads', label: 'downloads', icon: Download },
    { id: 'connection', label: 'connection', icon: Network },
    { id: 'speed', label: 'speed', icon: Gauge },
    { id: 'queue', label: 'queue', icon: ListOrdered },
    { id: 'web', label: 'web UI', icon: Globe },
    { id: 'appearance', label: 'appearance', icon: Palette }
  ]
  const themes: { id: ThemeChoice; label: string }[] = [
    { id: 'system', label: 'system' },
    { id: 'dark', label: 'dark' },
    { id: 'light', label: 'light' }
  ]
  const days = ['m', 't', 'w', 't', 'f', 's', 's']
  const dayNames = ['monday', 'tuesday', 'wednesday', 'thursday', 'friday', 'saturday', 'sunday']

  let tab = $state<Tab>('downloads')
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

  function setNumber(key: 'listen_port' | 'peer_limit' | 'max_active_downloads' | 'seed_minutes', value: string, min: number, max: number, label: string): void {
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
      <button type="button" class={['tab', tab === t.id && 'active']} aria-current={tab === t.id} onclick={() => (tab = t.id)}>
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
    {:else if tab === 'downloads'}
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
    {:else if tab === 'connection'}
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
      </div>
      {#if app.snapshot && !app.snapshot.totals.port}
        <p class="hint warn">bawkbit is not listening for incoming peers. another program may be using the port.</p>
      {/if}
    {:else if tab === 'speed'}
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
    {:else if tab === 'queue'}
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
    {:else if tab === 'web'}
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
    {:else if tab === 'appearance'}
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
