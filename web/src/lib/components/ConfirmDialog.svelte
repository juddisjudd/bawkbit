<script lang="ts">
  import { app, type Confirm } from '$lib/state.svelte'

  let { confirm }: { confirm: Confirm } = $props()

  function close(run?: () => void): void {
    app.confirm = null
    run?.()
  }

  function focus(node: HTMLElement): void {
    node.focus()
  }
</script>

<svelte:window onkeydown={(e) => e.key === 'Escape' && close()} />

<div class="backdrop" role="presentation" onmousedown={(e) => e.target === e.currentTarget && close()}>
  <div class={['dialog', confirm.danger && 'danger']} role="alertdialog" aria-modal="true" aria-label={confirm.message}>
    <p class="message">{confirm.message}</p>
    <div class="actions">
      <button type="button" class="btn ghost" onclick={() => close()}>cancel</button>
      {#if confirm.alt}
        {@const alt = confirm.alt}
        <button type="button" class={['btn', alt.danger && 'danger']} onclick={() => close(alt.run)}>{alt.label}</button>
      {/if}
      <button type="button" class={['btn', 'strong', confirm.danger && 'danger']} {@attach focus} onclick={() => close(confirm.run)}>{confirm.confirm}</button>
    </div>
  </div>
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
    gap: 18px;
    width: min(440px, 100%);
    padding: 20px;
    border: 1px solid var(--border-weak);
    border-radius: var(--radius-lg);
    background: var(--bg);
    box-shadow: var(--shadow);
  }
  .dialog.danger {
    border-color: color-mix(in oklch, var(--danger) 50%, transparent);
  }
  .message {
    color: var(--text-strong);
    overflow-wrap: anywhere;
    line-height: 1.6;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
