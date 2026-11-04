<script lang="ts">
  import CircleAlert from '@lucide/svelte/icons/circle-alert'
  import Pencil from '@lucide/svelte/icons/pencil'
  import Plus from '@lucide/svelte/icons/plus'
  import RefreshCw from '@lucide/svelte/icons/refresh-cw'
  import RssIcon from '@lucide/svelte/icons/rss'
  import Trash from '@lucide/svelte/icons/trash-2'
  import { api, feedLabel, newRule, type Feed, type FeedItem, type Rule, type RuleMatch } from '$lib/api'
  import { age, bytes } from '$lib/format'
  import { app, type RssTab } from '$lib/state.svelte'
  import Checkbox from './Checkbox.svelte'

  let { compact }: { compact: boolean } = $props()

  const tabs: { id: RssTab; label: string }[] = [
    { id: 'feeds', label: 'feeds' },
    { id: 'rules', label: 'rules' }
  ]

  let url = $state('')
  let adding = $state(false)
  /** What was changed in the open rule; every other field follows the server's copy. */
  let edits = $state<Partial<Rule>>({})
  let matches = $state.raw<RuleMatch[]>([])
  let matchError = $state('')
  let saving = $state(false)
  let previewAttempt = 0

  const feeds = $derived(app.rss?.feeds ?? [])
  const rules = $derived(app.rss?.rules ?? [])
  const feed = $derived(feeds.find((f) => f.id === app.rssFeed) ?? feeds[0] ?? null)
  const categories = $derived(app.snapshot?.categories ?? [])
  const labels = $derived(new Map(feeds.map((f) => [f.id, feedLabel(f)])))

  const draft = $derived.by((): Rule | null => {
    if (app.rssRule === null) return null
    if (app.rssRule === '') return { ...newRule(), ...edits }
    const stored = rules.find((r) => r.id === app.rssRule)
    return stored ? { ...stored, ...edits } : null
  })

  function edit<K extends keyof Rule>(key: K, value: Rule[K]): void {
    edits[key] = value
  }

  // asks the server, a moment after the last keystroke, what the rule being edited would take
  $effect(() => {
    if (!draft || !app.rss) return
    const rule = $state.snapshot(draft)
    const mine = ++previewAttempt
    const timer = setTimeout(async () => {
      try {
        const found = await api.ruleMatches(rule)
        if (mine !== previewAttempt) return
        matches = found
        matchError = ''
      } catch (err) {
        if (mine !== previewAttempt) return
        matches = []
        matchError = (err as Error).message
      }
    }, 300)
    return () => clearTimeout(timer)
  })

  async function addFeed(event: SubmitEvent): Promise<void> {
    event.preventDefault()
    const address = url.trim()
    if (!address) return
    adding = true
    const before = new Set(feeds.map((f) => f.id))
    const ok = await app.rssAct({ action: 'add_feed', url: address })
    adding = false
    if (!ok) return
    url = ''
    const added = app.rss?.feeds.find((f) => !before.has(f.id))
    if (added) app.rssFeed = added.id
  }

  function refresh(id: string | null): void {
    void app.rssAct({ action: 'refresh', id }, id ? 'checking the feed' : 'checking every feed')
  }

  function rename(f: Feed): void {
    app.prompt = {
      placeholder: 'feed name; empty uses its own',
      value: f.name,
      submit: 'rename',
      run: (name) => void app.rssAct({ action: 'update_feed', id: f.id, name: name.trim(), enabled: f.enabled })
    }
  }

  function setEnabled(f: Feed, enabled: boolean): void {
    void app.rssAct({ action: 'update_feed', id: f.id, name: f.name, enabled })
  }

  function removeFeed(f: Feed): void {
    app.confirm = {
      message: `remove ${feedLabel(f)}? rules that watch only this feed are turned off.`,
      confirm: 'remove',
      danger: true,
      run: () => {
        if (app.rssFeed === f.id) app.rssFeed = null
        void app.rssAct({ action: 'remove_feed', id: f.id })
      }
    }
  }

  function feedMenu(f: Feed, event: MouseEvent): void {
    event.preventDefault()
    app.menu = {
      x: event.clientX,
      y: event.clientY,
      items: [
        { label: 'refresh', action: () => refresh(f.id) },
        { label: 'rename…', action: () => rename(f) },
        { label: 'on', checked: f.enabled, action: () => setEnabled(f, !f.enabled) },
        { label: 'copy address', action: () => void app.copy(f.url, 'address') },
        'sep',
        { label: 'remove', danger: true, action: () => removeFeed(f) }
      ]
    }
  }

  function addItem(item: FeedItem): void {
    if (item.link) app.openAdd({ link: item.link })
  }

  function itemMenu(item: FeedItem, event: MouseEvent): void {
    event.preventDefault()
    app.menu = {
      x: event.clientX,
      y: event.clientY,
      items: [
        { label: 'add', disabled: !item.link, action: () => addItem(item) },
        { label: 'copy link', disabled: !item.link, action: () => void app.copy(item.link, 'link') },
        { label: 'open page', disabled: !item.page, action: () => item.page && window.open(item.page, '_blank', 'noopener,noreferrer') }
      ]
    }
  }

  function meta(item: FeedItem): string {
    return [item.size ? bytes(item.size) : '', age(item.published), item.rule ? `taken by ${item.rule}` : ''].filter(Boolean).join(' · ')
  }

  function openRule(rule: Rule | null): void {
    app.rssRule = rule?.id ?? ''
    edits = {}
    matches = []
    matchError = ''
  }

  function toggleFeed(id: string): void {
    if (!draft) return
    edit('feeds', draft.feeds.includes(id) ? draft.feeds.filter((f) => f !== id) : [...draft.feeds, id])
  }

  async function saveRule(event: SubmitEvent): Promise<void> {
    event.preventDefault()
    if (!draft) return
    saving = true
    const before = new Set(rules.map((r) => r.id))
    const fresh = !draft.id
    const ok = await app.rssAct({ action: 'save_rule', rule: $state.snapshot(draft) }, 'rule saved')
    saving = false
    if (!ok) return
    if (fresh) {
      const added = app.rss?.rules.find((r) => !before.has(r.id))
      if (added) app.rssRule = added.id
    }
    edits = {}
  }

  function removeRule(): void {
    const rule = draft
    if (!rule?.id) return
    app.confirm = {
      message: `remove the rule ${rule.name || 'without a name'}?`,
      confirm: 'remove',
      danger: true,
      run: async () => {
        if (await app.rssAct({ action: 'remove_rule', id: rule.id })) {
          app.rssRule = null
          edits = {}
        }
      }
    }
  }
</script>

<section class={['rss', compact && 'compact']} aria-label="rss">
  <div class="top">
    <div class="seg" role="group" aria-label="view">
      {#each tabs as t (t.id)}
        <button type="button" class={{ active: app.rssTab === t.id }} aria-pressed={app.rssTab === t.id} onclick={() => (app.rssTab = t.id)}>{t.label}</button>
      {/each}
    </div>
    {#if app.rssTab === 'feeds'}
      <form class="add" onsubmit={addFeed}>
        <input class="input" type="url" placeholder="feed address" aria-label="feed address" spellcheck="false" autocomplete="off" bind:value={url} />
        <button type="submit" class="btn" disabled={adding || !url.trim()}><Plus />add</button>
      </form>
      {#if feeds.length}
        <button type="button" class="btn ghost icon" title="check every feed now" aria-label="check every feed now" onclick={() => refresh(null)}><RefreshCw /></button>
      {/if}
    {:else}
      <button type="button" class="btn" onclick={() => openRule(null)}><Plus />new rule</button>
    {/if}
  </div>

  {#if app.rssError}
    <p class="error" role="alert">{app.rssError}</p>
  {/if}

  {#if !app.rss}
    <p class="nothing muted">{app.rssError ? '' : 'loading…'}</p>
  {:else if app.rssTab === 'feeds'}
    {#if !feeds.length}
      <div class="empty">
        <RssIcon size={28} />
        <p>add the address of a torrent site's RSS feed. rules then add new items for you.</p>
      </div>
    {:else}
      <div class="split">
        <div class="list" role="listbox" aria-label="feeds">
          {#each feeds as f (f.id)}
            {@const active = feed?.id === f.id}
            <button
              type="button"
              role="option"
              aria-selected={active}
              class={['entry', active && 'active', !f.enabled && 'off']}
              title={f.url}
              onclick={() => (app.rssFeed = f.id)}
              oncontextmenu={(e) => feedMenu(f, e)}
            >
              <span class="truncate grow">{feedLabel(f)}</span>
              {#if f.error}<CircleAlert size={13} class="warn" aria-label="the last check failed" />{/if}
              <span class="n num">{f.items.length || ''}</span>
            </button>
          {/each}
        </div>

        {#if feed}
          <div class="pane">
            <div class="pane-head">
              <div class="heading">
                <span class="strong truncate" title={feedLabel(feed)}>{feedLabel(feed)}</span>
                <span class="muted small truncate" title={feed.url}>{feed.url}</span>
              </div>
              <Checkbox checked={feed.enabled} label="on" onchange={(v) => setEnabled(feed, v)} />
              <button type="button" class="btn ghost icon" title="check now" aria-label="check now" onclick={() => refresh(feed.id)}><RefreshCw /></button>
              <button type="button" class="btn ghost icon" title="rename" aria-label="rename" onclick={() => rename(feed)}><Pencil /></button>
              <button type="button" class="btn ghost icon" title="remove" aria-label="remove" onclick={() => removeFeed(feed)}><Trash /></button>
            </div>
            {#if feed.error}
              <p class="error small" role="alert">{feed.error}</p>
            {:else if feed.checked}
              <p class="muted small">checked {age(feed.checked)} ago</p>
            {:else}
              <p class="muted small">checking…</p>
            {/if}
            {#if feed.items.length}
              <div class="items" role="group" aria-label="items">
                {#each feed.items as item (item.id)}
                  <button
                    type="button"
                    class={['item', !item.link && 'nolink']}
                    title={item.link ? item.title : `${item.title}\nthe feed gives no torrent for this one`}
                    disabled={!item.link}
                    onclick={() => addItem(item)}
                    oncontextmenu={(e) => itemMenu(item, e)}
                  >
                    <span class="lead" aria-hidden="true"><Plus size={14} class="plus" /></span>
                    <span class="name">
                      <span class="title truncate">{item.title || item.link}</span>
                      <span class="meta num truncate">{meta(item)}</span>
                    </span>
                  </button>
                {/each}
              </div>
            {:else if feed.checked}
              <p class="nothing muted">the feed lists nothing.</p>
            {/if}
          </div>
        {/if}
      </div>
    {/if}
  {:else}
    <div class="split">
      <div class="list" role="listbox" aria-label="rules">
        {#each rules as rule (rule.id)}
          {@const active = draft?.id === rule.id}
          <button type="button" role="option" aria-selected={active} class={['entry', active && 'active', !rule.enabled && 'off']} onclick={() => openRule(rule)}>
            <span class="truncate grow">{rule.name}</span>
            {#if rule.last_match}<span class="n num" title="last took something {age(rule.last_match)} ago">{age(rule.last_match)}</span>{/if}
          </button>
        {:else}
          <p class="muted small pad">no rules yet.</p>
        {/each}
      </div>

      {#if draft}
        <form class="pane editor" onsubmit={saveRule}>
          <div class="pane-head">
            <input class="input grow" placeholder="name" aria-label="rule name" spellcheck="false" value={draft.name} oninput={(e) => edit('name', e.currentTarget.value)} />
            <Checkbox checked={draft.enabled} label="on" onchange={(v) => edit('enabled', v)} />
          </div>
          <input class="input" placeholder="must contain" aria-label="must contain" spellcheck="false" value={draft.must_contain} oninput={(e) => edit('must_contain', e.currentTarget.value)} />
          <input class="input" placeholder="must not contain" aria-label="must not contain" spellcheck="false" value={draft.must_not_contain} oninput={(e) => edit('must_not_contain', e.currentTarget.value)} />
          {#if matchError}
            <p class="error small" role="alert">{matchError}</p>
          {:else}
            <p class="hint">
              {draft.regex ? 'regular expressions, ignoring case.' : 'every word must be in the title. | separates alternatives and * matches anything.'}
            </p>
          {/if}
          <div class="checks">
            <Checkbox checked={draft.regex} label="regular expressions" onchange={(v) => edit('regex', v)} />
            <Checkbox checked={draft.smart_episodes} label="each episode once" onchange={(v) => edit('smart_episodes', v)} />
            <span class="hint indent">skips S01E02 when it was taken already in another quality.</span>
          </div>

          {#if feeds.length}
            <div class="field">
              <span class="label">feeds</span>
              <div class="feeds">
                {#each feeds as f (f.id)}
                  <Checkbox checked={draft.feeds.includes(f.id)} label={feedLabel(f)} onchange={() => toggleFeed(f.id)} />
                {/each}
              </div>
              <span class="hint">none picked watches every feed.</span>
            </div>
          {/if}

          <div class="pair">
            <input
              class="input"
              placeholder="category"
              aria-label="category"
              list="rss-categories"
              spellcheck="false"
              value={draft.category ?? ''}
              oninput={(e) => edit('category', e.currentTarget.value || null)}
            />
            <input
              class="input"
              placeholder="folder"
              aria-label="save to folder"
              title="empty uses the category's folder"
              spellcheck="false"
              value={draft.save_path ?? ''}
              oninput={(e) => edit('save_path', e.currentTarget.value || null)}
            />
          </div>
          <datalist id="rss-categories">
            {#each categories as c (c.name)}<option value={c.name}></option>{/each}
          </datalist>
          <div class="checks">
            <Checkbox checked={draft.paused} label="add paused" onchange={(v) => edit('paused', v)} />
          </div>

          <div class="actions">
            {#if draft.id}
              <button type="button" class="btn ghost danger" onclick={removeRule}><Trash />remove</button>
            {/if}
            <span class="grow"></span>
            <button type="submit" class="btn strong" disabled={saving || !draft.name.trim() || !!matchError}>save</button>
          </div>

          <div class="field">
            <span class="label">matches now · {matches.length}</span>
            <span class="hint">a rule takes new items as they arrive. add any of these by hand.</span>
            {#if matches.length}
              <div class="items matches">
                {#each matches as m (`${m.feed}:${m.item.id}`)}
                  <button type="button" class="item" title={m.item.title} onclick={() => addItem(m.item)} oncontextmenu={(e) => itemMenu(m.item, e)}>
                    <span class="lead" aria-hidden="true"><Plus size={14} class="plus" /></span>
                    <span class="name">
                      <span class="title truncate">{m.item.title}</span>
                      <span class="meta num truncate">{[labels.get(m.feed), meta(m.item)].filter(Boolean).join(' · ')}</span>
                    </span>
                  </button>
                {/each}
              </div>
            {/if}
          </div>
        </form>
      {:else}
        <div class="empty">
          <p>pick a rule, or make a new one to add matching items as feeds list them.</p>
        </div>
      {/if}
    </div>
  {/if}
</section>

<style>
  .rss {
    container-type: inline-size;
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
    min-height: 0;
  }
  .top {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    padding: 12px;
    flex: none;
  }
  .add {
    display: flex;
    flex: 1;
    gap: 8px;
    max-width: 560px;
  }
  .add .input {
    flex: 1;
    min-width: 0;
  }
  .seg {
    display: flex;
    border: 1px solid var(--border-weak);
    border-radius: var(--radius);
    overflow: hidden;
  }
  .seg button {
    height: 30px;
    padding: 0 12px;
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
  .split {
    display: grid;
    grid-template-columns: 240px minmax(0, 1fr);
    flex: 1;
    min-height: 0;
    border-top: 1px solid var(--border-weak);
  }
  .list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 8px;
    border-right: 1px solid var(--border-weak);
    overflow-y: auto;
  }
  .entry {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 30px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--radius);
    background: none;
    color: var(--text);
    text-align: left;
    cursor: pointer;
  }
  .entry:hover {
    background: var(--bg-weak);
  }
  .entry.active {
    background: var(--bg-weak-hover);
    color: var(--text-strong);
  }
  .entry.off {
    color: var(--text-weaker);
  }
  .entry :global(.warn) {
    flex: none;
    color: var(--warning);
  }
  .grow {
    flex: 1;
    min-width: 0;
  }
  .n {
    color: var(--text-weak);
    font-size: 12px;
  }
  .pad {
    padding: 6px 10px;
  }
  .pane {
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-width: 0;
    min-height: 0;
    padding: 12px;
    overflow-y: auto;
  }
  .pane > * {
    flex: none;
  }
  .editor {
    max-width: 640px;
  }
  .pane-head {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .heading {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .small {
    font-size: 12px;
  }
  .error {
    color: var(--danger);
    font-size: 12px;
  }
  .top + .error {
    padding: 0 12px 8px;
  }
  .hint {
    color: var(--text-weak);
    font-size: 12px;
  }
  .hint.indent {
    margin: -4px 0 0 28px;
  }
  .checks {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: 6px;
  }
  .label {
    color: var(--text-weak);
    font-size: 12px;
  }
  .feeds {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 16px;
  }
  .pair {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .btn.danger {
    color: var(--danger);
  }
  .items {
    display: flex;
    flex-direction: column;
    border-top: 1px solid var(--border-weak);
  }
  .item {
    display: grid;
    grid-template-columns: 30px minmax(0, 1fr);
    align-items: center;
    min-height: 44px;
    padding: 4px 8px 4px 0;
    border: 0;
    border-bottom: 1px solid color-mix(in oklch, var(--border-weak) 55%, transparent);
    background: none;
    color: inherit;
    text-align: left;
    cursor: pointer;
  }
  .item:hover:not(:disabled),
  .item:focus-visible {
    background: var(--bg-weak);
    outline: none;
  }
  .item:disabled {
    cursor: default;
  }
  .lead {
    display: flex;
    justify-content: center;
    color: var(--text-weaker);
  }
  .lead :global(.plus) {
    opacity: 0;
  }
  .item:hover:not(:disabled) .lead :global(.plus),
  .item:focus-visible .lead :global(.plus) {
    opacity: 1;
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
  .nolink .title {
    color: var(--text-weak);
  }
  .meta {
    color: var(--text-weak);
    font-size: 12px;
  }
  .nothing {
    padding: 32px;
    text-align: center;
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
  .empty :global(svg) {
    color: var(--text-weaker);
  }
  @container (max-width: 640px) {
    .split {
      grid-template-columns: minmax(0, 1fr);
      grid-template-rows: auto minmax(0, 1fr);
    }
    .list {
      max-height: 180px;
      border-right: 0;
      border-bottom: 1px solid var(--border-weak);
    }
    .pair {
      grid-template-columns: 1fr;
    }
  }
  .compact .top {
    padding: 10px 8px;
  }
  .compact .item {
    min-height: 52px;
  }
</style>
