// Mirrors crates/core/src/api.rs and crates/core/src/settings.rs; keep the two in step.

export type Id = string

export type State =
  | 'metadata'
  | 'checking'
  | 'downloading'
  | 'stalled'
  | 'seeding'
  | 'done'
  | 'paused'
  | 'queued'
  | 'error'

export interface Torrent {
  id: Id
  name: string
  state: State
  progress: number
  size: number
  done: number
  uploaded: number
  down: number
  up: number
  eta: number | null
  ratio: number
  peers: number
  seeds: number
  category: string | null
  tags: string[]
  added: number
  completed: number | null
  save_path: string
  error: string | null
  forced: boolean
  no_upload: boolean
  /** Bytes per second for this torrent alone; 0 means no limit. */
  down_limit: number
  up_limit: number
}

export interface Totals {
  down: number
  up: number
  down_limit: number
  up_limit: number
  alt_speed: boolean
  dht_nodes: number
  downloaded: number
  uploaded: number
  free_space: number | null
  port: number | null
}

export interface Category {
  name: string
  save_path: string
  /** Bytes per second shared by every torrent in it; 0 means no limit. */
  down_limit: number
  up_limit: number
}

export interface Snapshot {
  rev: number
  torrents: Torrent[]
  totals: Totals
  categories: Category[]
  tags: string[]
  history: [number, number][]
  /** Changes whenever the RSS feeds or rules do. */
  rss_rev: number
}

/** A torrent that finished downloading, sent as a `finished` event. */
export interface Finished {
  id: Id
  name: string
}

export interface TorrentFile {
  index: number
  path: string
  size: number
  done: number
  included: boolean
}

export interface Peer {
  addr: string
  client: string | null
  transport: string | null
  down: number
  up: number
  downloaded: number
  uploaded: number
}

export interface Details {
  id: Id
  name: string
  save_path: string
  magnet: string
  size: number
  piece_size: number
  pieces: number
  piece_map: number[]
  files: TorrentFile[]
  peers: Peer[]
  trackers: string[]
}

export type Source =
  | { kind: 'link'; value: string }
  | { kind: 'file'; value: string }
  | { kind: 'resolved'; value: Id }

export interface PreviewFile {
  index: number
  path: string
  size: number
}

export interface Preview {
  id: Id
  name: string
  size: number
  files: PreviewFile[]
  save_path: string
  existing: boolean
}

export interface AddRequest {
  source: Source
  save_path?: string | null
  category?: string | null
  tags?: string[]
  paused?: boolean | null
  files?: number[] | null
  /** Trusts that the files are complete, so only a spot check runs. */
  skip_check?: boolean
}

export type Action =
  | { action: 'pause'; ids: Id[] }
  | { action: 'resume'; ids: Id[] }
  | { action: 'remove'; ids: Id[]; delete_files?: boolean }
  | { action: 'set_files'; id: Id; files: number[] }
  | { action: 'set_category'; ids: Id[]; category: string | null }
  | { action: 'add_tags'; ids: Id[]; tags: string[] }
  | { action: 'remove_tags'; ids: Id[]; tags: string[] }
  | { action: 'save_category'; name: string; save_path?: string | null }
  | { action: 'remove_category'; name: string }
  | { action: 'remove_tag'; tag: string }
  | { action: 'set_alt_speed'; on: boolean }
  | { action: 'pause_all' }
  | { action: 'resume_all' }
  | { action: 'force_start'; ids: Id[] }
  | { action: 'recheck'; ids: Id[] }
  | { action: 'set_trackers'; id: Id; trackers: string[] }
  | { action: 'set_no_upload'; ids: Id[]; on: boolean }
  | { action: 'set_limits'; ids: Id[]; down: number; up: number }
  | { action: 'rename_category'; from: string; to: string }
  | { action: 'rename_tag'; from: string; to: string }
  | { action: 'set_category_limits'; name: string; down: number; up: number }

export type Reply =
  | { kind: 'done' }
  | { kind: 'added'; id: Id }
  | ({ kind: 'preview' } & Preview)
  | ({ kind: 'details' } & Details)
  | ({ kind: 'settings' } & Settings)

export interface Schedule {
  enabled: boolean
  from: number
  to: number
  days: number
}

export interface Web {
  enabled: boolean
  address: string
  port: number
  username: string
  password_hash: string
  password?: string | null
  local_bypass: boolean
}

export interface Settings {
  download_dir: string
  start_paused: boolean
  listen_port: number
  upnp: boolean
  dht: boolean
  peer_limit: number
  down_limit: number
  up_limit: number
  alt_down_limit: number
  alt_up_limit: number
  alt_speed: boolean
  schedule: Schedule
  max_active_downloads: number
  ratio_limit: number
  seed_minutes: number
  watch_dir: string
  utp: boolean
  /** Read only here: the server ignores changes from clients. */
  on_finish: string
  /** Minutes between feed checks; 0 checks only by hand. */
  rss_minutes: number
  web: Web
  indexers: Indexer[]
}

export type IndexerKind = 'prowlarr' | 'jackett'

export interface Indexer {
  id: string
  kind: IndexerKind
  name: string
  url: string
  /** Always empty from the server; send empty to keep the stored key. */
  api_key: string
  has_key: boolean
  enabled: boolean
}

export type SearchCategory = 'all' | 'movies' | 'tv' | 'music' | 'books' | 'software' | 'games' | 'other'

export const SEARCH_CATEGORIES: SearchCategory[] = ['all', 'movies', 'tv', 'music', 'books', 'software', 'games', 'other']

export interface SearchRequest {
  query: string
  category: SearchCategory
}

export interface SearchResult {
  title: string
  size: number
  seeds: number | null
  peers: number | null
  published: number | null
  source: string
  via: string
  link: string
  info_url: string | null
  info_hash: string | null
  category: SearchCategory
}

export interface SearchFailure {
  source: string
  error: string
}

export interface SearchReply {
  results: SearchResult[]
  failures: SearchFailure[]
}

export interface FeedItem {
  id: string
  title: string
  /** A magnet link or a link to the .torrent file; empty when the feed gave neither. */
  link: string
  page: string | null
  published: number | null
  size: number | null
  /** The name of the rule that added it. */
  rule: string | null
}

export interface Feed {
  id: string
  url: string
  /** A name given by hand; empty shows `title`. */
  name: string
  title: string
  enabled: boolean
  /** Newest first. */
  items: FeedItem[]
  checked: number | null
  error: string | null
  ready: boolean
}

export interface Rule {
  id: string
  name: string
  enabled: boolean
  must_contain: string
  must_not_contain: string
  regex: boolean
  smart_episodes: boolean
  /** Feed ids it watches; empty watches every feed. */
  feeds: string[]
  category: string | null
  save_path: string | null
  paused: boolean
  episodes: string[]
  last_match: number | null
}

export interface Rss {
  feeds: Feed[]
  rules: Rule[]
}

export type RssAction =
  | { action: 'add_feed'; url: string }
  | { action: 'update_feed'; id: string; name: string; enabled: boolean }
  | { action: 'remove_feed'; id: string }
  | { action: 'refresh'; id?: string | null }
  | { action: 'save_rule'; rule: Rule }
  | { action: 'remove_rule'; id: string }

export interface RuleMatch {
  feed: string
  item: FeedItem
}

export interface Backup {
  app: string
  version: string
  settings: Settings
  categories: Category[]
  tags: string[]
  feeds: Feed[]
  rules: Rule[]
}

export function feedLabel(feed: Feed): string {
  return [feed.name, feed.title, feed.url].map((text) => text.trim()).find(Boolean) ?? ''
}

export function newRule(): Rule {
  return {
    id: '',
    name: '',
    enabled: true,
    must_contain: '',
    must_not_contain: '',
    regex: false,
    smart_episodes: false,
    feeds: [],
    category: null,
    save_path: null,
    paused: false,
    episodes: [],
    last_match: null
  }
}

export interface SessionInfo {
  signed_in: boolean
  has_password: boolean
  version: string
}

export class ApiError extends Error {
  status: number

  constructor(status: number, message: string) {
    super(message)
    this.status = status
  }
}

let onUnauthorized: () => void = () => {}

export function whenUnauthorized(handler: () => void): void {
  onUnauthorized = handler
}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  let response: Response
  try {
    response = await fetch(`/api${path}`, {
      method,
      credentials: 'same-origin',
      headers: body === undefined ? undefined : { 'content-type': 'application/json' },
      body: body === undefined ? undefined : JSON.stringify(body)
    })
  } catch {
    throw new ApiError(0, 'cannot reach bawkbit.')
  }
  if (response.status === 401 && path !== '/login') onUnauthorized()
  const text = await response.text()
  let data: unknown = null
  try {
    data = text ? JSON.parse(text) : null
  } catch {
    data = null
  }
  if (!response.ok) {
    const message =
      data && typeof data === 'object' && 'error' in data && typeof data.error === 'string'
        ? data.error
        : `bawkbit answered ${response.status}.`
    throw new ApiError(response.status, message)
  }
  return data as T
}

export const api = {
  session: () => request<SessionInfo>('GET', '/session'),
  login: (username: string, password: string) => request<{ ok: boolean }>('POST', '/login', { username, password }),
  logout: () => request<{ ok: boolean }>('POST', '/logout', {}),
  snapshot: () => request<Snapshot>('GET', '/snapshot'),
  details: (id: Id) => request<Details>('GET', `/torrents/${encodeURIComponent(id)}`),
  preview: (source: Source) => request<Preview>('POST', '/preview', source),
  add: (body: AddRequest) => request<{ id: Id }>('POST', '/torrents', body),
  act: (action: Action) => request<Reply>('POST', '/actions', action),
  settings: () => request<Settings>('GET', '/settings'),
  saveSettings: (settings: Settings) => request<Settings>('PUT', '/settings', settings),
  search: (body: SearchRequest) => request<SearchReply>('POST', '/search', body),
  testIndexer: (indexer: Indexer) => request<{ message: string }>('POST', '/indexers/test', indexer),
  rss: () => request<Rss>('GET', '/rss'),
  rssAct: (action: RssAction) => request<Rss>('POST', '/rss', action),
  ruleMatches: (rule: Rule) => request<RuleMatch[]>('POST', '/rss/matches', rule),
  backup: () => request<Backup>('GET', '/backup'),
  restore: (backup: unknown) => request<Reply>('POST', '/backup', backup)
}

export function fileToBase64(file: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => {
      const url = String(reader.result)
      resolve(url.slice(url.indexOf(',') + 1))
    }
    reader.onerror = () => reject(new Error('cannot read that file.'))
    reader.readAsDataURL(file)
  })
}

export function isLink(text: string): boolean {
  const value = text.trim()
  return /^magnet:\?/i.test(value) || /^https?:\/\/\S+$/i.test(value) || /^[0-9a-f]{40}$/i.test(value) || /^[a-z2-7]{32}$/i.test(value)
}
