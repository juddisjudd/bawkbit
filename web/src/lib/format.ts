const UNITS = ['B', 'KB', 'MB', 'GB', 'TB', 'PB']

export function bytes(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return '0 B'
  let value = n
  let unit = 0
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024
    unit += 1
  }
  if (unit === 0) return `${Math.round(value)} B`
  return `${value >= 100 ? value.toFixed(0) : value.toFixed(1)} ${UNITS[unit]}`
}

/** A speed, or nothing when there is none, so idle rows stay quiet. */
export function speed(n: number): string {
  return n > 0 ? `${bytes(n)}/s` : ''
}

export function eta(seconds: number | null): string {
  if (seconds === null || !Number.isFinite(seconds) || seconds <= 0) return ''
  if (seconds >= 100 * 86400) return '∞'
  const d = Math.floor(seconds / 86400)
  const h = Math.floor((seconds % 86400) / 3600)
  const m = Math.floor((seconds % 3600) / 60)
  const s = Math.floor(seconds % 60)
  if (d > 0) return `${d}d ${h}h`
  if (h > 0) return `${h}h ${m}m`
  if (m > 0) return `${m}m ${s}s`
  return `${s}s`
}

export function ratio(r: number): string {
  if (!Number.isFinite(r)) return '∞'
  return r.toFixed(2)
}

export function percent(fraction: number): string {
  const value = Math.max(0, Math.min(1, fraction)) * 100
  if (value >= 100) return '100%'
  return `${value < 10 ? value.toFixed(1) : Math.floor(value)}%`
}

export function count(n: number): string {
  return n.toLocaleString('en-US')
}

export function date(unix: number | null): string {
  if (!unix) return ''
  const d = new Date(unix * 1000)
  const now = new Date()
  const time = d.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' })
  if (d.toDateString() === now.toDateString()) return `today ${time}`
  const sameYear = d.getFullYear() === now.getFullYear()
  return d.toLocaleDateString(undefined, { day: 'numeric', month: 'short', year: sameYear ? undefined : 'numeric' }) + ` ${time}`
}

export function splitPath(path: string): [string, string] {
  const ix = path.lastIndexOf('/')
  return ix < 0 ? ['', path] : [path.slice(0, ix + 1), path.slice(ix + 1)]
}

/** Bytes per second as a number in KiB/s or MiB/s for a settings field, or '' for no limit. */
export function limitValue(bps: number, unit: 'k' | 'm'): string {
  if (!bps) return ''
  const value = bps / (unit === 'm' ? 1024 * 1024 : 1024)
  return String(Math.round(value * 100) / 100)
}

/** The unit a limit reads best in; MB/s for no limit, since most limits people set are in MB/s. */
export function limitUnit(bps: number): 'k' | 'm' {
  if (!bps) return 'm'
  return bps % (1024 * 1024) === 0 || bps >= 10 * 1024 * 1024 ? 'm' : 'k'
}

export function limitBytes(value: string, unit: 'k' | 'm'): number {
  const n = Number(value)
  if (!value.trim() || !Number.isFinite(n) || n <= 0) return 0
  return Math.round(n * (unit === 'm' ? 1024 * 1024 : 1024))
}

/** How long ago, in the largest unit that fits: 40s, 12m, 5h, 3d, 4mo, 2y. */
export function age(unix: number | null, now = Date.now() / 1000): string {
  if (!unix) return ''
  const seconds = Math.max(0, now - unix)
  if (seconds < 60) return `${Math.floor(seconds)}s`
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h`
  if (seconds < 86400 * 60) return `${Math.floor(seconds / 86400)}d`
  if (seconds < 86400 * 365) return `${Math.floor(seconds / (86400 * 30))}mo`
  return `${Math.floor(seconds / (86400 * 365))}y`
}
