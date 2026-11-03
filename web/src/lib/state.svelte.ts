import { SvelteSet } from 'svelte/reactivity'
import { api, isLink, whenUnauthorized, type Action, type Id, type SearchCategory, type SearchReply, type SearchResult, type SessionInfo, type Settings, type Snapshot, type Torrent } from './api'
import { COMPARE_RESULTS, parseRefine, refines, type SearchSortKey } from './search'
import { isRunning } from './states'

export type StatusFilter = 'all' | 'downloading' | 'seeding' | 'done' | 'paused' | 'active' | 'error'
export type SortKey = 'name' | 'size' | 'progress' | 'down' | 'up' | 'eta' | 'ratio' | 'peers' | 'added'
export type ThemeChoice = 'system' | 'dark' | 'light'
export type DetailsTab = 'overview' | 'files' | 'peers' | 'trackers'
export type View = 'list' | 'search' | 'settings'
export type SettingsTab = 'downloads' | 'connection' | 'speed' | 'queue' | 'search' | 'web' | 'appearance'

export interface MenuItem {
  label: string
  action?: () => void
  items?: () => (MenuItem | 'sep')[]
  checked?: boolean
  danger?: boolean
  disabled?: boolean
}

export interface Menu {
  x: number
  y: number
  items: (MenuItem | 'sep')[]
}

export interface Confirm {
  message: string
  confirm: string
  danger?: boolean
  run: () => void
  alt?: { label: string; danger?: boolean; run: () => void }
}

export interface Prompt {
  placeholder: string
  value: string
  submit: string
  run: (value: string) => void
}

export interface AddIntent {
  link?: string
  file?: File
  quick?: boolean
}

export interface Toast {
  id: number
  text: string
  error: boolean
}

const STATUS_FILTERS: Record<StatusFilter, (t: Torrent) => boolean> = {
  all: () => true,
  downloading: (t) => ['downloading', 'stalled', 'metadata', 'queued', 'checking'].includes(t.state) || (t.state === 'paused' && t.progress < 1),
  seeding: (t) => t.state === 'seeding',
  done: (t) => t.state === 'done' || t.state === 'seeding',
  paused: (t) => t.state === 'paused' || t.state === 'done',
  active: (t) => t.down > 0 || t.up > 0,
  error: (t) => t.state === 'error'
}

const COMPARE: Record<SortKey, (a: Torrent, b: Torrent) => number> = {
  name: (a, b) => a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: 'base' }),
  size: (a, b) => a.size - b.size,
  progress: (a, b) => a.progress - b.progress,
  down: (a, b) => a.down - b.down,
  up: (a, b) => a.up - b.up,
  eta: (a, b) => (a.eta ?? Infinity) - (b.eta ?? Infinity),
  ratio: (a, b) => a.ratio - b.ratio,
  peers: (a, b) => a.peers - b.peers,
  added: (a, b) => a.added - b.added
}

function stored<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(`bawkbit.${key}`)
    return raw === null ? fallback : (JSON.parse(raw) as T)
  } catch {
    return fallback
  }
}

function store(key: string, value: unknown): void {
  try {
    localStorage.setItem(`bawkbit.${key}`, JSON.stringify(value))
  } catch {
    // private browsing can refuse storage; the choice just won't survive a reload
  }
}

class App {
  phase = $state<'loading' | 'login' | 'ready'>('loading')
  session = $state.raw<SessionInfo | null>(null)
  snapshot = $state.raw<Snapshot | null>(null)
  live = $state<'connecting' | 'live' | 'reconnecting'>('connecting')
  view = $state<View>('list')
  settingsTab = $state<SettingsTab>('downloads')

  status = $state<StatusFilter>('all')
  category = $state<string | null>(null)
  tag = $state<string | null>(null)
  query = $state('')
  sort = $state<{ key: SortKey; desc: boolean }>(stored('sort', { key: 'added', desc: true }))

  selected = new SvelteSet<Id>()
  focused = $state<Id | null>(null)
  detailsOpen = $state(false)
  detailsTab = $state<DetailsTab>('overview')
  filtersOpen = $state(false)

  adding = $state<AddIntent | null>(null)
  menu = $state<Menu | null>(null)
  confirm = $state<Confirm | null>(null)
  prompt = $state<Prompt | null>(null)
  toasts = $state<Toast[]>([])
  dragging = $state(false)

  searchText = $state('')
  searchCategory = $state<SearchCategory>('all')
  searching = $state(false)
  searchReply = $state.raw<SearchReply | null>(null)
  searchError = $state('')
  refine = $state('')
  searchSort = $state<{ key: SearchSortKey; desc: boolean }>(stored('searchSort', { key: 'seeds', desc: true }))
  private searchAttempt = 0

  theme = $state<ThemeChoice>(stored('theme', 'system'))
  defaults = $state.raw<Settings | null>(null)

  torrents = $derived(this.snapshot?.torrents ?? [])

  visible = $derived.by(() => {
    const matches = STATUS_FILTERS[this.status]
    const words = this.query.trim().toLowerCase().split(/\s+/).filter(Boolean)
    const list = this.torrents.filter(
      (t) =>
        matches(t) &&
        (this.category === null || t.category === this.category) &&
        (this.tag === null || t.tags.includes(this.tag)) &&
        words.every((word) => t.name.toLowerCase().includes(word))
    )
    const compare = COMPARE[this.sort.key]
    const direction = this.sort.desc ? -1 : 1
    return list.sort((a, b) => direction * compare(a, b) || COMPARE.name(a, b))
  })

  counts = $derived.by(() => {
    const statuses = Object.fromEntries(
      (Object.keys(STATUS_FILTERS) as StatusFilter[]).map((key) => [key, this.torrents.filter(STATUS_FILTERS[key]).length])
    ) as Record<StatusFilter, number>
    const categories = new Map<string, number>()
    const tags = new Map<string, number>()
    for (const t of this.torrents) {
      if (t.category) categories.set(t.category, (categories.get(t.category) ?? 0) + 1)
      for (const tag of t.tags) tags.set(tag, (tags.get(tag) ?? 0) + 1)
    }
    return { statuses, categories, tags }
  })

  current = $derived(this.torrents.find((t) => t.id === this.focused) ?? null)

  /** Unknown until the settings load, so the search box shows rather than a false empty state. */
  hasIndexers = $derived(this.defaults === null || (this.defaults.indexers ?? []).some((indexer) => indexer.enabled))

  known = $derived(new Set(this.torrents.map((t) => t.id)))

  results = $derived.by(() => {
    const reply = this.searchReply
    if (!reply) return []
    const refine = parseRefine(this.refine)
    const compare = COMPARE_RESULTS[this.searchSort.key]
    const direction = this.searchSort.desc ? -1 : 1
    return reply.results.filter((r) => refines(r, refine)).sort((a, b) => direction * compare(a, b) || COMPARE_RESULTS.seeds(b, a))
  })

  private source: EventSource | null = null
  private retry: ReturnType<typeof setTimeout> | null = null
  private watchdog: ReturnType<typeof setInterval> | null = null
  private lastEvent = 0
  private anchor: Id | null = null
  private toastId = 0

  async start(): Promise<void> {
    whenUnauthorized(() => this.signedOut())
    this.applyTheme()
    window.matchMedia('(prefers-color-scheme: light)').addEventListener('change', () => this.applyTheme())
    await this.checkSession()
  }

  private async checkSession(): Promise<void> {
    try {
      this.session = await api.session()
    } catch {
      this.live = 'reconnecting'
      this.scheduleRetry()
      return
    }
    if (!this.session.signed_in) {
      this.signedOut()
      return
    }
    this.phase = 'ready'
    this.connect()
    this.loadDefaults()
  }

  signedOut(): void {
    this.source?.close()
    this.source = null
    if (this.retry) clearTimeout(this.retry)
    if (this.watchdog) clearInterval(this.watchdog)
    this.watchdog = null
    this.phase = 'login'
    this.snapshot = null
    this.selected.clear()
    this.focused = null
    this.detailsOpen = false
    this.view = 'list'
  }

  async login(username: string, password: string): Promise<void> {
    await api.login(username, password)
    await this.checkSession()
  }

  async logout(): Promise<void> {
    try {
      await api.logout()
    } finally {
      this.signedOut()
    }
  }

  private connect(): void {
    this.source?.close()
    const source = new EventSource('/api/events')
    this.source = source
    this.lastEvent = Date.now()
    source.addEventListener('snapshot', (event) => {
      this.lastEvent = Date.now()
      this.snapshot = JSON.parse((event as MessageEvent<string>).data) as Snapshot
      this.live = 'live'
    })
    source.onerror = () => this.lost(source)
    if (this.watchdog) clearInterval(this.watchdog)
    // a proxy can hold a dead stream open, so silence counts as a lost connection; snapshots come every second
    this.watchdog = setInterval(() => {
      if (this.source === source && Date.now() - this.lastEvent > 6000) this.lost(source)
    }, 2000)
  }

  private lost(source: EventSource): void {
    source.close()
    if (this.source !== source) return
    this.source = null
    if (this.watchdog) clearInterval(this.watchdog)
    this.watchdog = null
    this.live = 'reconnecting'
    this.scheduleRetry()
  }

  private scheduleRetry(): void {
    if (this.retry) clearTimeout(this.retry)
    this.retry = setTimeout(() => {
      this.retry = null
      void this.checkSession()
    }, 2000)
  }

  async loadDefaults(): Promise<void> {
    try {
      this.defaults = await api.settings()
    } catch {
      // the add dialog falls back to the server's own defaults
    }
  }

  setTheme(theme: ThemeChoice): void {
    this.theme = theme
    store('theme', theme)
    this.applyTheme()
  }

  private applyTheme(): void {
    const light = window.matchMedia('(prefers-color-scheme: light)').matches
    const resolved = this.theme === 'system' ? (light ? 'light' : 'dark') : this.theme
    document.documentElement.dataset.theme = resolved
    document.querySelector('meta[name="theme-color"]')?.setAttribute('content', resolved === 'light' ? '#fdfbfb' : '#13100f')
  }

  setSort(key: SortKey): void {
    this.sort = this.sort.key === key ? { key, desc: !this.sort.desc } : { key, desc: key !== 'name' && key !== 'eta' }
    store('sort', this.sort)
  }

  filterBy(status: StatusFilter, category: string | null = null, tag: string | null = null): void {
    this.view = 'list'
    this.status = status
    this.category = category
    this.tag = tag
    this.filtersOpen = false
  }

  click(id: Id, event: { ctrlKey: boolean; metaKey: boolean; shiftKey: boolean }): void {
    if (event.shiftKey && this.anchor) {
      const ids = this.visible.map((t) => t.id)
      const from = ids.indexOf(this.anchor)
      const to = ids.indexOf(id)
      if (from >= 0 && to >= 0) {
        if (!event.ctrlKey && !event.metaKey) this.selected.clear()
        for (const rangeId of ids.slice(Math.min(from, to), Math.max(from, to) + 1)) this.selected.add(rangeId)
        this.focused = id
        return
      }
    }
    if (event.ctrlKey || event.metaKey) {
      if (this.selected.has(id)) this.selected.delete(id)
      else this.selected.add(id)
    } else {
      this.selected.clear()
      this.selected.add(id)
    }
    this.anchor = id
    this.focused = id
  }

  selectAll(): void {
    for (const t of this.visible) this.selected.add(t.id)
  }

  clearSelection(): void {
    this.selected.clear()
    this.anchor = null
  }

  move(step: number, extend: boolean): void {
    const ids = this.visible.map((t) => t.id)
    if (!ids.length) return
    const at = this.focused ? ids.indexOf(this.focused) : -1
    const next = ids[Math.max(0, Math.min(ids.length - 1, at < 0 ? 0 : at + step))]
    if (extend) this.click(next, { ctrlKey: false, metaKey: false, shiftKey: true })
    else this.click(next, { ctrlKey: false, metaKey: false, shiftKey: false })
  }

  /** The torrents an action applies to: the selection, or the one in focus. */
  targets(): Id[] {
    if (this.selected.size) return [...this.selected]
    return this.focused ? [this.focused] : []
  }

  openDetails(id: Id): void {
    this.focused = id
    this.detailsOpen = true
  }

  async act(action: Action, success?: string): Promise<boolean> {
    try {
      await api.act(action)
      if (success) this.toast(success)
      return true
    } catch (error) {
      this.toast((error as Error).message, true)
      return false
    }
  }

  pause(ids: Id[]): void {
    if (ids.length) void this.act({ action: 'pause', ids })
  }

  resume(ids: Id[]): void {
    if (ids.length) void this.act({ action: 'resume', ids })
  }

  toggle(ids: Id[]): void {
    const chosen = this.torrents.filter((t) => ids.includes(t.id))
    if (chosen.some(isRunning)) this.pause(ids)
    else this.resume(ids)
  }

  private async removeNow(ids: Id[], deleteFiles: boolean): Promise<void> {
    const ok = await this.act({ action: 'remove', ids, delete_files: deleteFiles })
    if (!ok) return
    for (const id of ids) this.selected.delete(id)
    if (this.focused && ids.includes(this.focused)) {
      this.focused = null
      this.detailsOpen = false
    }
  }

  remove(ids: Id[], deleteFiles: boolean): void {
    if (!ids.length) return
    const run = () => this.removeNow(ids, deleteFiles)
    if (!deleteFiles) {
      void run()
      return
    }
    const name = ids.length === 1 ? (this.torrents.find((t) => t.id === ids[0])?.name ?? 'this torrent') : `${ids.length} torrents`
    this.confirm = {
      message: `remove ${name} and delete its downloaded files?`,
      confirm: 'delete files',
      danger: true,
      run: () => void run()
    }
  }

  /** Asks before removing, offering to delete the files too. */
  askRemove(ids: Id[]): void {
    if (!ids.length) return
    const name = ids.length === 1 ? (this.torrents.find((t) => t.id === ids[0])?.name ?? 'this torrent') : `${ids.length} torrents`
    this.confirm = {
      message: `remove ${name}?`,
      confirm: 'remove',
      run: () => this.remove(ids, false),
      alt: { label: 'delete files too', danger: true, run: () => void this.removeNow(ids, true) }
    }
  }

  setAltSpeed(on: boolean): void {
    void this.act({ action: 'set_alt_speed', on })
  }

  openAdd(intent: AddIntent = {}): void {
    this.adding = intent
  }

  openSettings(tab: SettingsTab): void {
    this.settingsTab = tab
    this.view = 'settings'
    this.filtersOpen = false
  }

  openSearch(): void {
    this.view = 'search'
    this.filtersOpen = false
  }

  async search(): Promise<void> {
    const query = this.searchText.trim()
    if (!query) return
    const mine = ++this.searchAttempt
    this.searching = true
    this.searchError = ''
    try {
      const reply = await api.search({ query, category: this.searchCategory })
      if (mine !== this.searchAttempt) return
      this.searchReply = reply
      this.refine = ''
    } catch (error) {
      if (mine !== this.searchAttempt) return
      this.searchError = (error as Error).message
      this.searchReply = null
    } finally {
      if (mine === this.searchAttempt) this.searching = false
    }
  }

  setSearchSort(key: SearchSortKey): void {
    this.searchSort = this.searchSort.key === key ? { key, desc: !this.searchSort.desc } : { key, desc: key !== 'title' && key !== 'source' }
    store('searchSort', this.searchSort)
  }

  /** Shows a torrent already in the list. */
  reveal(id: Id): void {
    this.filterBy('all')
    this.click(id, { ctrlKey: false, metaKey: false, shiftKey: false })
  }

  pickResult(result: SearchResult): void {
    if (result.info_hash && this.known.has(result.info_hash)) this.reveal(result.info_hash)
    else this.openAdd({ link: result.link })
  }

  toast(text: string, error = false): void {
    const id = ++this.toastId
    this.toasts = [...this.toasts, { id, text, error }]
    setTimeout(() => this.dismiss(id), error ? 6000 : 3000)
  }

  dismiss(id: number): void {
    this.toasts = this.toasts.filter((t) => t.id !== id)
  }

  async copy(text: string, what: string): Promise<void> {
    try {
      await navigator.clipboard.writeText(text)
      this.toast(`copied ${what}`)
    } catch {
      this.toast('the browser blocked the clipboard. it needs https or localhost.', true)
    }
  }

  handlePaste(text: string): boolean {
    if (!isLink(text)) return false
    this.openAdd({ link: text.trim() })
    return true
  }
}

export const app = new App()
