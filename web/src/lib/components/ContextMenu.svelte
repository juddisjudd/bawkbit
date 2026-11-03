<script lang="ts">
  import Check from '@lucide/svelte/icons/check'
  import ChevronLeft from '@lucide/svelte/icons/chevron-left'
  import ChevronRight from '@lucide/svelte/icons/chevron-right'
  import { app, type MenuItem } from '$lib/state.svelte'

  let el = $state<HTMLDivElement>()
  let stack = $state<{ label: string; items: (MenuItem | 'sep')[] }[]>([])

  const items = $derived(stack.length ? stack[stack.length - 1].items : (app.menu?.items ?? []))

  const position = $derived.by(() => {
    const menu = app.menu
    if (!menu) return { x: 0, y: 0 }
    const width = 220
    const height = (items.length + (stack.length ? 1 : 0)) * 30 + 8
    return {
      x: Math.max(8, Math.min(menu.x, window.innerWidth - width - 8)),
      y: Math.max(8, Math.min(menu.y, window.innerHeight - height - 8))
    }
  })

  function close(): void {
    app.menu = null
    stack = []
  }

  function pick(item: MenuItem): void {
    if (item.items) {
      stack = [...stack, { label: item.label, items: item.items() }]
      return
    }
    close()
    item.action?.()
  }

  function focusFirst(node: HTMLDivElement): void {
    node.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus()
  }

  function keys(event: KeyboardEvent): void {
    if (!app.menu) return
    if (event.key === 'Escape') {
      event.preventDefault()
      if (stack.length) stack = stack.slice(0, -1)
      else close()
      return
    }
    if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return
    event.preventDefault()
    const buttons = [...(el?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)') ?? [])]
    const at = buttons.indexOf(document.activeElement as HTMLButtonElement)
    const next = event.key === 'ArrowDown' ? (at + 1) % buttons.length : (at - 1 + buttons.length) % buttons.length
    buttons[next]?.focus()
  }
</script>

<svelte:window
  onmousedown={(e) => app.menu && !el?.contains(e.target as Node) && close()}
  onkeydown={keys}
  onblur={close}
  onresize={close}
/>

{#if app.menu}
  {#key stack.length}
    <div class="menu" role="menu" tabindex="-1" bind:this={el} {@attach focusFirst} style:left="{position.x}px" style:top="{position.y}px">
      {#if stack.length}
        <button type="button" role="menuitem" class="back" onclick={() => (stack = stack.slice(0, -1))}>
          <ChevronLeft size={14} />
          <span>{stack[stack.length - 1].label}</span>
        </button>
        <div class="sep"></div>
      {/if}
      {#each items as item, i (i)}
        {#if item === 'sep'}
          <div class="sep"></div>
        {:else}
          <button
            type="button"
            role={item.checked === undefined ? 'menuitem' : 'menuitemcheckbox'}
            aria-checked={item.checked}
            class={[item.danger && 'danger']}
            disabled={item.disabled}
            onclick={() => pick(item)}
          >
            <span class="check">{#if item.checked}<Check size={13} />{/if}</span>
            <span class="label truncate">{item.label}</span>
            {#if item.items}<ChevronRight size={14} class="more" />{/if}
          </button>
        {/if}
      {/each}
    </div>
  {/key}
{/if}

<style>
  .menu {
    position: fixed;
    z-index: 150;
    width: 220px;
    padding: 4px;
    border: 1px solid var(--border-weak);
    border-radius: var(--radius);
    background: var(--bg);
    box-shadow: var(--shadow);
  }
  .menu:focus {
    outline: none;
  }
  button {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    height: 28px;
    padding: 0 8px 0 4px;
    border: 0;
    border-radius: 3px;
    background: none;
    color: var(--text-strong);
    text-align: left;
    cursor: pointer;
  }
  button:hover:not(:disabled),
  button:focus-visible {
    background: var(--bg-weak-hover);
    outline: none;
  }
  button:disabled {
    color: var(--text-weaker);
    cursor: default;
  }
  .check {
    display: inline-flex;
    width: 16px;
    flex: none;
    justify-content: center;
    color: var(--yolk);
  }
  .label {
    flex: 1;
  }
  .back {
    padding-left: 6px;
    color: var(--text-weak);
  }
  .danger {
    color: var(--danger);
  }
  .sep {
    height: 1px;
    margin: 4px 0;
    background: var(--border-weak);
  }
  button :global(.more) {
    color: var(--text-weaker);
  }
</style>
