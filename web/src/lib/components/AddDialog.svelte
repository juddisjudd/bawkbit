<script lang="ts">
  import FilePlus from '@lucide/svelte/icons/file-plus'
  import LoaderCircle from '@lucide/svelte/icons/loader-circle'
  import X from '@lucide/svelte/icons/x'
  import { untrack } from 'svelte'
  import { api, fileToBase64, isLink, type Preview, type Source } from '$lib/api'
  import { bytes, splitPath } from '$lib/format'
  import { app, type AddIntent } from '$lib/state.svelte'

  let { intent }: { intent: AddIntent } = $props()

  // svelte-ignore state_referenced_locally
  let link = $state(intent.link ?? '')
  let step = $state<'input' | 'resolving' | 'preview'>('input')
  let error = $state('')
  let busy = $state(false)
  let preview = $state.raw<Preview | null>(null)
  let chosen = $state.raw(new Set<number>())
  let savePath = $state('')
  let pathEdited = $state(false)
  let category = $state('')
  let tags = $state('')
  let start = $state(!app.defaults?.start_paused)
  let skipCheck = $state(false)
  let fileInput = $state<HTMLInputElement>()
  let over = $state(false)
  let attempt = 0

  const categories = $derived(app.snapshot?.categories ?? [])
  const chosenSize = $derived(preview ? preview.files.filter((f) => chosen.has(f.index)).reduce((sum, f) => sum + f.size, 0) : 0)
  const magnet = $derived(/^magnet:\?/i.test(link.trim()) || /^[0-9a-f]{40}$/i.test(link.trim()))

  function close(): void {
    attempt += 1
    app.adding = null
  }

  function categoryPath(name: string): string | null {
    const base = app.defaults?.download_dir ?? preview?.save_path ?? ''
    if (!name.trim()) return preview?.save_path ?? base
    const known = categories.find((c) => c.name === name.trim())
    if (known?.save_path) return known.save_path
    if (!base) return null
    const sep = base.includes('\\') && !base.includes('/') ? '\\' : '/'
    return `${base.replace(/[\\/]+$/, '')}${sep}${name.trim()}`
  }

  function onCategory(): void {
    if (pathEdited) return
    const path = categoryPath(category)
    if (path !== null) savePath = path
  }

  async function resolve(source: Source): Promise<void> {
    const mine = ++attempt
    error = ''
    step = 'resolving'
    try {
      const result = await api.preview(source)
      if (mine !== attempt) return
      preview = result
      chosen = new Set(result.files.map((f) => f.index))
      savePath = result.save_path
      pathEdited = false
      step = 'preview'
    } catch (err) {
      if (mine !== attempt) return
      error = (err as Error).message
      step = 'input'
    }
  }

  async function useFile(file: File | undefined): Promise<void> {
    if (!file) return
    if (!file.name.toLowerCase().endsWith('.torrent')) {
      error = 'that is not a .torrent file.'
      return
    }
    try {
      await resolve({ kind: 'file', value: await fileToBase64(file) })
    } catch (err) {
      error = (err as Error).message
    }
  }

  function next(event?: SubmitEvent): void {
    event?.preventDefault()
    if (!isLink(link)) {
      error = 'paste a magnet link, an info hash, or a link to a .torrent file.'
      return
    }
    void resolve({ kind: 'link', value: link.trim() })
  }

  async function quickAdd(): Promise<void> {
    if (!isLink(link)) {
      error = 'paste a magnet link first.'
      return
    }
    busy = true
    try {
      await api.add({ source: { kind: 'link', value: link.trim() } })
      app.toast('added. it shows up once peers send its details.')
      close()
    } catch (err) {
      error = (err as Error).message
    } finally {
      busy = false
    }
  }

  async function add(event: SubmitEvent): Promise<void> {
    event.preventDefault()
    if (!preview) return
    if (!chosen.size) {
      error = 'pick at least one file.'
      return
    }
    busy = true
    error = ''
    try {
      const { id } = await api.add({
        source: { kind: 'resolved', value: preview.id },
        save_path: savePath.trim() || null,
        category: category.trim() || null,
        tags: tags.split(',').map((t) => t.trim()).filter(Boolean),
        paused: !start,
        files: chosen.size === preview.files.length ? null : [...chosen].sort((a, b) => a - b),
        skip_check: skipCheck
      })
      app.click(id, { ctrlKey: false, metaKey: false, shiftKey: false })
      close()
    } catch (err) {
      error = (err as Error).message
    } finally {
      busy = false
    }
  }

  function toggle(index: number): void {
    const next = new Set(chosen)
    if (next.has(index)) next.delete(index)
    else next.add(index)
    chosen = next
  }

  function toggleAll(): void {
    if (!preview) return
    chosen = chosen.size === preview.files.length ? new Set() : new Set(preview.files.map((f) => f.index))
  }

  function showExisting(): void {
    if (!preview) return
    app.filterBy('all')
    app.click(preview.id, { ctrlKey: false, metaKey: false, shiftKey: false })
    close()
  }

  function focus(node: HTMLElement): void {
    node.focus()
  }

  function drop(event: DragEvent): void {
    event.preventDefault()
    event.stopPropagation()
    over = false
    const file = event.dataTransfer?.files[0]
    if (file) {
      void useFile(file)
      return
    }
    const text = event.dataTransfer?.getData('text/plain') ?? ''
    if (isLink(text)) {
      link = text.trim()
      next()
    }
  }

  function begin(): void {
    untrack(() => {
      if (intent.file) void useFile(intent.file)
      else if (intent.link) next()
    })
  }
</script>

<svelte:window onkeydown={(e) => e.key === 'Escape' && !app.menu && close()} />

<div class="backdrop" role="presentation" onmousedown={(e) => e.target === e.currentTarget && close()}>
  <div
    class={['dialog', over && 'over', step === 'preview' && 'wide']}
    role="dialog"
    aria-modal="true"
    aria-label="add torrent"
    tabindex="-1"
    {@attach begin}
    ondragover={(e) => {
      e.preventDefault()
      e.stopPropagation()
      over = true
    }}
    ondragleave={() => (over = false)}
    ondrop={drop}
  >
    <button type="button" class="btn ghost icon close" aria-label="close" onclick={close}><X /></button>

    {#if step === 'input'}
      <form class="stack" onsubmit={next}>
        <input
          class="input"
          placeholder="magnet link or URL"
          aria-label="magnet link or URL"
          spellcheck="false"
          autocomplete="off"
          bind:value={link}
          {@attach focus}
        />
        <p class="error" role="alert">{error}</p>
        <div class="row">
          <button type="button" class="btn ghost" onclick={() => fileInput?.click()}><FilePlus />.torrent file</button>
          <span class="grow"></span>
          {#if magnet}
            <button type="button" class="btn ghost" disabled={busy} title="add with default settings, without looking at the files first" onclick={quickAdd}>add now</button>
          {/if}
          <button type="submit" class="btn strong" disabled={!link.trim()}>next</button>
        </div>
        <input
          bind:this={fileInput}
          type="file"
          accept=".torrent,application/x-bittorrent"
          hidden
          onchange={(e) => void useFile(e.currentTarget.files?.[0])}
        />
      </form>
    {:else if step === 'resolving'}
      <div class="stack resolving" role="status">
        <p class="center"><LoaderCircle size={16} class="spin" /> asking peers for the file list…</p>
        <p class="muted center small">a new magnet link can take a minute.</p>
        <div class="row">
          <span class="grow"></span>
          <button type="button" class="btn ghost" onclick={() => ((step = 'input'), (attempt += 1))}>back</button>
          {#if magnet}
            <button type="button" class="btn" disabled={busy} onclick={quickAdd}>add without waiting</button>
          {/if}
        </div>
      </div>
    {:else if preview}
      <form class="stack" onsubmit={add}>
        <div class="title">
          <span class="strong name" title={preview.name}>{preview.name}</span>
          <span class="muted num">{bytes(preview.size)}{preview.files.length > 1 ? ` · ${preview.files.length} files` : ''}</span>
        </div>

        {#if preview.existing}
          <p class="muted">already in the list.</p>
          <div class="row">
            <span class="grow"></span>
            <button type="button" class="btn strong" onclick={showExisting} {@attach focus}>show it</button>
          </div>
        {:else}
          {#if preview.files.length > 1}
            <div class="files" role="group" aria-label="files to download">
              <div class="file head">
                <button
                  type="button"
                  class="check"
                  role="checkbox"
                  aria-checked={chosen.size === preview.files.length ? 'true' : chosen.size === 0 ? 'false' : 'mixed'}
                  aria-label="every file"
                  onclick={toggleAll}
                >[{chosen.size === preview.files.length ? 'x' : chosen.size === 0 ? ' ' : '-'}]</button>
                <span class="muted small">{chosen.size === preview.files.length ? 'every file' : `${chosen.size} of ${preview.files.length}`}</span>
                <span class="muted small num">{bytes(chosenSize)}</span>
              </div>
              <div class="list">
                {#each preview.files as file (file.index)}
                  {@const [dir, name] = splitPath(file.path)}
                  <button type="button" class={['file', !chosen.has(file.index) && 'off']} role="checkbox" aria-checked={chosen.has(file.index)} onclick={() => toggle(file.index)}>
                    <span class="check">[{chosen.has(file.index) ? 'x' : ' '}]</span>
                    <span class="truncate path" title={file.path}><span class="muted">{dir}</span>{name}</span>
                    <span class="muted num">{bytes(file.size)}</span>
                  </button>
                {/each}
              </div>
            </div>
          {/if}

          <input class="input" aria-label="save to folder" title="save to folder" placeholder="folder" spellcheck="false" bind:value={savePath} oninput={() => (pathEdited = true)} />
          <div class="pair">
            <input class="input" aria-label="category" placeholder="category" list="bawkbit-categories" spellcheck="false" bind:value={category} oninput={onCategory} />
            <input class="input" aria-label="tags" placeholder="tags, comma separated" spellcheck="false" bind:value={tags} />
          </div>
          <datalist id="bawkbit-categories">
            {#each categories as c (c.name)}<option value={c.name}></option>{/each}
          </datalist>

          {#if skipCheck}
            <p class="muted small">the files must already be complete in the folder. a spot check still runs.</p>
          {/if}
          <p class="error" role="alert">{error}</p>
          <div class="row">
            <button type="button" class="check toggle" role="checkbox" aria-checked={start} onclick={() => (start = !start)}>[{start ? 'x' : ' '}] start now</button>
            <button
              type="button"
              class="check toggle"
              role="checkbox"
              aria-checked={skipCheck}
              title="the files are already complete; a spot check still runs"
              onclick={() => (skipCheck = !skipCheck)}>[{skipCheck ? 'x' : ' '}] skip check</button
            >
            <span class="grow"></span>
            <button type="button" class="btn ghost" onclick={close}>cancel</button>
            <button type="submit" class="btn strong" disabled={busy || chosen.size === 0} {@attach focus}>add</button>
          </div>
        {/if}
      </form>
    {/if}
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 100;
    display: grid;
    place-items: center;
    padding: 16px;
    background: oklch(0 0 0 / 0.45);
  }
  .dialog {
    position: relative;
    width: min(520px, 100%);
    max-height: calc(100dvh - 32px);
    padding: 22px 22px 18px;
    border: 1px solid var(--border-weak);
    border-radius: var(--radius-lg);
    background: var(--bg);
    box-shadow: var(--shadow);
    overflow: auto;
  }
  .dialog:focus {
    outline: none;
  }
  .dialog.wide {
    width: min(640px, 100%);
  }
  .dialog.over {
    border-color: var(--yolk);
    background: color-mix(in oklch, var(--bg) 92%, var(--yolk));
  }
  .close {
    position: absolute;
    top: 8px;
    right: 8px;
  }
  .stack {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .stack > .input:first-child {
    margin-right: 30px;
    width: calc(100% - 30px);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .grow {
    flex: 1;
  }
  .error {
    min-height: 1.5em;
    color: var(--danger);
    font-size: 12px;
  }
  .error:empty {
    min-height: 0;
  }
  .center {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    text-align: center;
  }
  .small {
    font-size: 12px;
  }
  .resolving {
    padding-top: 12px;
    gap: 6px;
  }
  .resolving .row {
    margin-top: 14px;
  }
  .title {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding-right: 30px;
  }
  .name {
    overflow-wrap: anywhere;
    font-weight: 500;
  }
  .files {
    border: 1px solid var(--border-weak);
    border-radius: var(--radius);
  }
  .list {
    max-height: 240px;
    overflow-y: auto;
  }
  .file {
    display: grid;
    grid-template-columns: 30px minmax(0, 1fr) auto;
    align-items: center;
    gap: 8px;
    width: 100%;
    min-height: 28px;
    padding: 0 10px;
    border: 0;
    background: none;
    color: var(--text-strong);
    text-align: left;
    cursor: pointer;
  }
  .file:hover {
    background: var(--bg-weak);
  }
  .file.head {
    border-bottom: 1px solid var(--border-weak);
    cursor: default;
  }
  .file.head:hover {
    background: none;
  }
  .file.off,
  .file.off .muted {
    color: var(--text-weaker);
  }
  .check {
    padding: 0;
    border: 0;
    background: none;
    color: var(--text-strong);
    white-space: pre;
    cursor: pointer;
    text-align: left;
  }
  .toggle {
    color: var(--text);
  }
  .pair {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px;
  }
  @media (max-width: 520px) {
    .pair {
      grid-template-columns: 1fr;
    }
    .row {
      flex-wrap: wrap;
    }
  }
</style>
