<script lang="ts">
  import X from '@lucide/svelte/icons/x'
  import { app } from '$lib/state.svelte'
</script>

<div class="toasts" role="status" aria-live="polite">
  {#each app.toasts as toast (toast.id)}
    <div class={['toast', toast.error && 'error']}>
      <span class="dot"></span>
      <span class="text">{toast.text}</span>
      <button type="button" class="btn ghost small icon" aria-label="dismiss" onclick={() => app.dismiss(toast.id)}><X /></button>
    </div>
  {/each}
</div>

<style>
  .toasts {
    position: fixed;
    right: 16px;
    bottom: 16px;
    z-index: 200;
    display: flex;
    flex-direction: column;
    gap: 8px;
    width: min(380px, calc(100vw - 32px));
    pointer-events: none;
  }
  .toast {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 6px 8px 12px;
    border: 1px solid var(--border-weak);
    border-radius: var(--radius);
    background: var(--bg-weak);
    box-shadow: var(--shadow);
    color: var(--text-strong);
    pointer-events: auto;
  }
  .dot {
    background: var(--success);
  }
  .error .dot {
    background: var(--danger);
  }
  .text {
    flex: 1;
    overflow-wrap: anywhere;
  }
</style>
