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
}

export interface Snapshot {
  rev: number
  torrents: Torrent[]
  totals: Totals
  categories: Category[]
  tags: string[]
  history: [number, number][]
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
  web: Web
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
  saveSettings: (settings: Settings) => request<Settings>('PUT', '/settings', settings)
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
