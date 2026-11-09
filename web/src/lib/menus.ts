import { api, type Id } from './api'
import { app, type MenuItem } from './state.svelte'
import { isRunning } from './states'

function categoryItems(ids: Id[]): (MenuItem | 'sep')[] {
  const chosen = app.torrents.filter((t) => ids.includes(t.id))
  const all = (name: string | null) => chosen.length > 0 && chosen.every((t) => t.category === name)
  const set = (category: string | null) => void app.act({ action: 'set_category', ids, category })
  return [
    { label: 'none', checked: all(null), action: () => set(null) },
    ...(app.snapshot?.categories ?? []).map((c) => ({ label: c.name, checked: all(c.name), action: () => set(c.name) })),
    'sep',
    {
      label: 'new category…',
      action: () =>
        (app.prompt = { placeholder: 'category name', value: '', submit: 'set', run: (name) => name.trim() && set(name.trim()) })
    }
  ]
}

function tagItems(ids: Id[]): (MenuItem | 'sep')[] {
  const chosen = app.torrents.filter((t) => ids.includes(t.id))
  const tags = app.snapshot?.tags ?? []
  return [
    ...tags.map((tag) => {
      const on = chosen.length > 0 && chosen.every((t) => t.tags.includes(tag))
      return {
        label: tag,
        checked: on,
        action: () => void app.act(on ? { action: 'remove_tags', ids, tags: [tag] } : { action: 'add_tags', ids, tags: [tag] })
      }
    }),
    ...(tags.length ? (['sep'] as const) : []),
    {
      label: 'new tag…',
      action: () =>
        (app.prompt = {
          placeholder: 'tags, separated by commas',
          value: '',
          submit: 'add',
          run: (value) => {
            const fresh = value.split(',').map((tag) => tag.trim()).filter(Boolean)
            if (fresh.length) void app.act({ action: 'add_tags', ids, tags: fresh })
          }
        })
    }
  ]
}

function copyItems(ids: Id[]): MenuItem[] {
  const chosen = app.torrents.filter((t) => ids.includes(t.id))
  const many = chosen.length > 1
  return [
    {
      label: many ? 'magnet links' : 'magnet link',
      action: async () => {
        try {
          const links = await Promise.all(chosen.map(async (t) => (await api.details(t.id)).magnet))
          await app.copy(links.join('\n'), many ? 'magnet links' : 'magnet link')
        } catch (error) {
          app.toast((error as Error).message, true)
        }
      }
    },
    { label: many ? 'info hashes' : 'info hash', action: () => app.copy(chosen.map((t) => t.id).join('\n'), many ? 'info hashes' : 'info hash') },
    { label: many ? 'names' : 'name', action: () => app.copy(chosen.map((t) => t.name).join('\n'), many ? 'names' : 'name') }
  ]
}

/** The right-click menu for the selected torrents. */
export function openTorrentMenu(x: number, y: number): void {
  const ids = app.targets()
  if (!ids.length) return
  const chosen = app.torrents.filter((t) => ids.includes(t.id))
  const running = chosen.some(isRunning)
  const stopped = chosen.some((t) => !isRunning(t))
  const forced = chosen.length > 0 && chosen.every((t) => t.forced)
  const quiet = chosen.length > 0 && chosen.every((t) => t.no_upload)
  const one = ids.length === 1 ? ids[0] : null
  app.menu = {
    x,
    y,
    items: [
      { label: 'resume', disabled: !stopped, action: () => app.resume(ids) },
      { label: 'pause', disabled: !running, action: () => app.pause(ids) },
      { label: 'force start', checked: forced, action: () => (forced ? app.resume(ids) : app.forceStart(ids)) },
      { label: 're-check', action: () => app.recheck(ids) },
      'sep',
      { label: 'category', items: () => categoryItems(ids) },
      { label: 'tags', items: () => tagItems(ids) },
      'sep',
      { label: 'no upload', checked: quiet, action: () => app.setNoUpload(ids, !quiet) },
      { label: 'speed limits…', action: () => app.editLimits(ids) },
      ...(one ? [{ label: 'add trackers…', action: () => (app.addingTrackers = one) }] : []),
      'sep',
      ...(one ? [{ label: 'details', action: () => app.openDetails(one) }] : []),
      { label: 'copy', items: () => copyItems(ids) },
      'sep',
      { label: 'remove', action: () => app.askRemove(ids) },
      { label: 'remove and delete files', danger: true, action: () => app.remove(ids, true) }
    ]
  }
}
