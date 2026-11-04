<script lang="ts">
  import type { Id } from '$lib/api'
  import { app } from '$lib/state.svelte'

  let { id }: { id: Id } = $props()

  let text = $state('')
  let busy = $state(false)

  const name = $derived(app.torrents.find((t) => t.id === id)?.name ?? '')

  async function submit(event: SubmitEvent): Promise<void> {
    event.preventDefault()
    busy = true
    const ok = await app.addTrackers(id, text)
    busy = false
    if (ok) {
      app.addingTrackers = null
      app.toast('trackers added')
    }
  }

  function keys(event: KeyboardEvent): void {
    if (event.key === 'Enter' && (event.ctrlKey || event.metaKey)) {
      event.preventDefault()
      ;(event.currentTarget as HTMLTextAreaElement).form?.requestSubmit()
    }
  }
</script>

<svelte:window onkeydown={(e) => e.key === 'Escape' && (app.addingTrackers = null)} />

<div class="backdrop" role="presentation" onmousedown={(e) => e.target === e.currentTarget && (app.addingTrackers = null)}>
  <form class="dialog" aria-label="add trackers to {name}" onsubmit={submit}>
    <p class="strong truncate">{name}</p>
    <textarea
      class="input"
      rows="6"
      placeholder="tracker addresses, one per line"
      aria-label="tracker addresses, one per line"
      spellcheck="false"
      bind:value={text}
      onkeydown={keys}
      {@attach (node: HTMLTextAreaElement) => node.focus()}
    ></textarea>
    <div class="actions">
      <button type="button" class="btn ghost" onclick={() => (app.addingTrackers = null)}>cancel</button>
      <button type="submit" class="btn strong" disabled={busy || !text.trim()}>add</button>
    </div>
  </form>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 120;
    display: grid;
    place-items: center;
    padding: 16px;
    background: oklch(0 0 0 / 0.45);
  }
  .dialog {
    display: flex;
    flex-direction: column;
    gap: 14px;
    width: min(520px, 100%);
    padding: 20px;
    border: 1px solid var(--border-weak);
    border-radius: var(--radius-lg);
    background: var(--bg);
    box-shadow: var(--shadow);
  }
  .dialog p {
    margin: 0;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
