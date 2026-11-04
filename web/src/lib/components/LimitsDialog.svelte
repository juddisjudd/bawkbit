<script lang="ts">
  import { limitBytes, limitUnit, limitValue } from '$lib/format'
  import { app, type LimitsEdit } from '$lib/state.svelte'

  let { edit }: { edit: LimitsEdit } = $props()

  type Way = 'down' | 'up'

  // svelte-ignore state_referenced_locally
  let units = $state<Record<Way, 'k' | 'm'>>({ down: limitUnit(edit.down), up: limitUnit(edit.up) })
  // svelte-ignore state_referenced_locally
  let values = $state<Record<Way, string>>({ down: limitValue(edit.down, units.down), up: limitValue(edit.up, units.up) })
  let busy = $state(false)

  const count = $derived(edit.ids.length)

  async function submit(event: SubmitEvent): Promise<void> {
    event.preventDefault()
    busy = true
    const ok = await app.setLimits(edit.ids, limitBytes(values.down, units.down), limitBytes(values.up, units.up))
    busy = false
    if (ok) app.limits = null
  }

  function focus(node: HTMLInputElement): void {
    node.focus()
    node.select()
  }
</script>

<svelte:window onkeydown={(e) => e.key === 'Escape' && (app.limits = null)} />

<div class="backdrop" role="presentation" onmousedown={(e) => e.target === e.currentTarget && (app.limits = null)}>
  <form class="dialog" aria-label="speed limits" onsubmit={submit}>
    {#if count > 1}<p class="muted">for {count} torrents</p>{/if}
    {@render field('down', 'download')}
    {@render field('up', 'upload')}
    <div class="actions">
      <button type="button" class="btn ghost" onclick={() => (app.limits = null)}>cancel</button>
      <button type="submit" class="btn strong" disabled={busy}>set</button>
    </div>
  </form>
</div>

{#snippet field(way: Way, label: string)}
  <div class="row">
    <label for="torrent-limit-{way}">{label}</label>
    <input
      id="torrent-limit-{way}"
      class="input num"
      type="number"
      min="0"
      step="any"
      placeholder="no limit"
      value={values[way]}
      oninput={(e) => (values[way] = e.currentTarget.value)}
      {@attach (node: HTMLInputElement) => {
        if (way === 'down') focus(node)
      }}
    />
    <span class="seg" role="group" aria-label="{label} unit">
      <button type="button" class={{ active: units[way] === 'k' }} aria-pressed={units[way] === 'k'} onclick={() => (units[way] = 'k')}>KB/s</button>
      <button type="button" class={{ active: units[way] === 'm' }} aria-pressed={units[way] === 'm'} onclick={() => (units[way] = 'm')}>MB/s</button>
    </span>
  </div>
{/snippet}

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
    width: min(400px, 100%);
    padding: 20px;
    border: 1px solid var(--border-weak);
    border-radius: var(--radius-lg);
    background: var(--bg);
    box-shadow: var(--shadow);
  }
  .dialog p {
    margin: 0;
    font-size: 12px;
  }
  .row {
    display: grid;
    grid-template-columns: 80px minmax(0, 1fr) auto;
    align-items: center;
    gap: 10px;
  }
  .row label {
    color: var(--text);
  }
  .seg {
    display: flex;
    border: 1px solid var(--border-weak);
    border-radius: var(--radius);
    overflow: hidden;
  }
  .seg button {
    height: 30px;
    padding: 0 10px;
    border: 0;
    border-left: 1px solid var(--border-weak);
    background: none;
    color: var(--text-weak);
    cursor: pointer;
  }
  .seg button:first-child {
    border-left: 0;
  }
  .seg button:hover {
    color: var(--text-strong);
  }
  .seg button.active {
    background: var(--bg-strong);
    color: var(--text-inverted);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
