<script lang="ts">
  import AddDialog from '$lib/components/AddDialog.svelte'
  import ConfirmDialog from '$lib/components/ConfirmDialog.svelte'
  import ContextMenu from '$lib/components/ContextMenu.svelte'
  import Details from '$lib/components/Details.svelte'
  import Header from '$lib/components/Header.svelte'
  import Login from '$lib/components/Login.svelte'
  import PromptDialog from '$lib/components/PromptDialog.svelte'
  import SearchPage from '$lib/components/SearchPage.svelte'
  import Settings from '$lib/components/Settings.svelte'
  import Sidebar from '$lib/components/Sidebar.svelte'
  import Toasts from '$lib/components/Toasts.svelte'
  import TorrentList from '$lib/components/TorrentList.svelte'
  import Logo from '$lib/components/Logo.svelte'
  import { app } from '$lib/state.svelte'

  let width = $state(1280)
  let list = $state<ReturnType<typeof TorrentList>>()
  let searchPage = $state<ReturnType<typeof SearchPage>>()
  let dragDepth = 0

  const compact = $derived(width <= 720)
  const overlay = $derived(app.adding !== null || app.confirm !== null || app.prompt !== null || app.menu !== null)

  void app.start()

  function typing(target: EventTarget | null): boolean {
    return target instanceof HTMLElement && (target.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName))
  }

  function keys(event: KeyboardEvent): void {
    if (app.phase !== 'ready' || overlay) return
    if (event.key === 'Escape') {
      if (app.filtersOpen) app.filtersOpen = false
      else if (app.detailsOpen) app.detailsOpen = false
      else if (app.view === 'settings') app.view = 'list'
      else if (app.view === 'search') {
        if (!typing(event.target)) app.view = 'list'
      } else app.clearSelection()
      return
    }
    if (typing(event.target)) return
    if (app.view === 'search' && event.key === '/') {
      event.preventDefault()
      searchPage?.focusBox()
      return
    }
    if (app.view !== 'list') return
    if (event.key === '/' || ((event.ctrlKey || event.metaKey) && event.key === 'f')) {
      event.preventDefault()
      list?.focusFilter()
    } else if ((event.ctrlKey || event.metaKey) && event.key === 'a') {
      event.preventDefault()
      app.selectAll()
    } else if (event.key === 'n' && !event.ctrlKey && !event.metaKey && !event.altKey) {
      event.preventDefault()
      app.openAdd()
    }
  }

  function paste(event: ClipboardEvent): void {
    if (app.phase !== 'ready' || overlay || typing(event.target)) return
    const file = [...(event.clipboardData?.files ?? [])].find((f) => f.name.toLowerCase().endsWith('.torrent'))
    if (file) {
      event.preventDefault()
      app.openAdd({ file })
      return
    }
    const text = event.clipboardData?.getData('text/plain') ?? ''
    if (app.handlePaste(text)) event.preventDefault()
  }

  function hasFiles(event: DragEvent): boolean {
    return [...(event.dataTransfer?.types ?? [])].includes('Files')
  }

  function dragEnter(event: DragEvent): void {
    if (app.phase !== 'ready' || app.adding || !hasFiles(event)) return
    dragDepth += 1
    app.dragging = true
  }

  function dragLeave(): void {
    dragDepth = Math.max(0, dragDepth - 1)
    if (dragDepth === 0) app.dragging = false
  }

  function dragOver(event: DragEvent): void {
    if (app.dragging) event.preventDefault()
  }

  function drop(event: DragEvent): void {
    if (!app.dragging) return
    event.preventDefault()
    dragDepth = 0
    app.dragging = false
    const files = [...(event.dataTransfer?.files ?? [])].filter((f) => f.name.toLowerCase().endsWith('.torrent'))
    if (!files.length) {
      app.toast('drop a .torrent file.', true)
      return
    }
    app.openAdd({ file: files[0] })
    if (files.length > 1) app.toast('one file at a time for now: added the first.', true)
  }
</script>

<svelte:window bind:innerWidth={width} onkeydown={keys} onpaste={paste} ondragenter={dragEnter} ondragleave={dragLeave} ondragover={dragOver} ondrop={drop} />

{#if app.phase === 'loading'}
  <div class="splash" aria-busy="true"><Logo size={18} /></div>
{:else if app.phase === 'login'}
  <Login />
{:else}
  <div class="app">
    <Header />
    <div class="body">
      {#if app.view === 'settings'}
        <main class="main"><Settings /></main>
      {:else}
        <Sidebar />
        <main class="main">
          {#if app.view === 'search'}
            <SearchPage bind:this={searchPage} {compact} />
          {:else}
            <TorrentList bind:this={list} {compact} />
            {#if app.detailsOpen && app.focused}
              <Details {compact} />
            {/if}
          {/if}
        </main>
      {/if}
    </div>
  </div>

  {#if app.dragging}
    <div class="drop" aria-hidden="true"><Logo size={22} word={false} /><span>drop to add</span></div>
  {/if}
  {#if app.adding}<AddDialog intent={app.adding} />{/if}
  {#if app.confirm}<ConfirmDialog confirm={app.confirm} />{/if}
  {#if app.prompt}<PromptDialog prompt={app.prompt} />{/if}
  <ContextMenu />
{/if}
<Toasts />

<style>
  .splash {
    display: grid;
    place-items: center;
    height: 100%;
    opacity: 0.6;
  }
  .app {
    display: flex;
    flex-direction: column;
    height: 100%;
  }
  .body {
    display: flex;
    flex: 1;
    min-height: 0;
  }
  .main {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
    min-height: 0;
  }
  .drop {
    position: fixed;
    inset: 8px;
    z-index: 90;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    border: 2px dashed var(--yolk);
    border-radius: var(--radius-lg);
    background: color-mix(in oklch, var(--bg) 85%, transparent);
    color: var(--text-strong);
    pointer-events: none;
  }
  .drop :global(.mark) {
    fill: var(--yolk);
  }
</style>
