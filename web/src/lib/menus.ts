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

/** The right-click menu for the selected torrents. */
export function openTorrentMenu(x: number, y: number): void {
  const ids = app.targets()
  if (!ids.length) return
  const chosen = app.torrents.filter((t) => ids.includes(t.id))
  const running = chosen.some(isRunning)
  const stopped = chosen.some((t) => !isRunning(t))
  const one = ids.length === 1 ? ids[0] : null
  app.menu = {
    x,
    y,
    items: [
      { label: 'resume', disabled: !stopped, action: () => app.resume(ids) },
      { label: 'pause', disabled: !running, action: () => app.pause(ids) },
      'sep',
      { label: 'category', items: () => categoryItems(ids) },
      { label: 'tags', items: () => tagItems(ids) },
      'sep',
      ...(one
        ? [
            { label: 'details', action: () => app.openDetails(one) },
            {
              label: 'copy magnet link',
              action: async () => {
                try {
                  const details = await api.details(one)
                  await app.copy(details.magnet, 'magnet link')
                } catch (error) {
                  app.toast((error as Error).message, true)
                }
              }
            },
            'sep' as const
          ]
        : []),
      { label: 'remove', action: () => app.remove(ids, false) },
      { label: 'remove and delete files', danger: true, action: () => app.remove(ids, true) }
    ]
  }
}
