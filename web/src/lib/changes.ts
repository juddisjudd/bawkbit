import type { Changes, Snapshot, Torrent } from './api'

/** Speed samples the engine keeps. */
const HISTORY = 300

/** The snapshot `old` becomes once `changes` apply; rows that did not change keep their objects. */
export function applyChanges(old: Snapshot, changes: Changes): Snapshot {
  const fresh = new Map(changes.torrents.map((t) => [t.id, t]))
  const gone = new Set(changes.removed)
  const torrents: Torrent[] = []
  for (const t of old.torrents) {
    if (gone.has(t.id)) continue
    torrents.push(fresh.get(t.id) ?? t)
    fresh.delete(t.id)
  }
  torrents.push(...fresh.values())
  return {
    rev: changes.rev,
    torrents,
    totals: changes.totals,
    categories: changes.categories ?? old.categories,
    tags: changes.tags ?? old.tags,
    history: changes.history.length ? [...old.history, ...changes.history].slice(-HISTORY) : old.history,
    rss_rev: changes.rss_rev
  }
}
