<script lang="ts">
  import { app, type Prompt } from '$lib/state.svelte'

  let { prompt }: { prompt: Prompt } = $props()

  // svelte-ignore state_referenced_locally
  let value = $state(prompt.value)

  function submit(event: SubmitEvent): void {
    event.preventDefault()
    const run = prompt.run
    // the prop follows app.prompt, so it reads null once that is cleared
    app.prompt = null
    run(value)
  }

  function focus(node: HTMLInputElement): void {
    node.focus()
    node.select()
  }
</script>

<svelte:window onkeydown={(e) => e.key === 'Escape' && (app.prompt = null)} />

<div class="backdrop" role="presentation" onmousedown={(e) => e.target === e.currentTarget && (app.prompt = null)}>
  <form class="dialog" aria-label={prompt.placeholder} onsubmit={submit}>
    <input class="input" placeholder={prompt.placeholder} aria-label={prompt.placeholder} spellcheck="false" bind:value {@attach focus} />
    <div class="actions">
      <button type="button" class="btn ghost" onclick={() => (app.prompt = null)}>cancel</button>
      <button type="submit" class="btn strong">{prompt.submit}</button>
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
    width: min(420px, 100%);
    padding: 20px;
    border: 1px solid var(--border-weak);
    border-radius: var(--radius-lg);
    background: var(--bg);
    box-shadow: var(--shadow);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
