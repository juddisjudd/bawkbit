<script lang="ts">
  import source from '../../assets/mark.svg?raw'

  let { size = 14, mark = true, word = true }: { size?: number; mark?: boolean; word?: boolean } = $props()

  const d = source.match(/\sd="([^"]+)"/)?.[1] ?? ''
</script>

<span class="logo" style:font-size="{size}px">
  {#if mark}
    <svg class="mark" viewBox="40 12 432 488" aria-hidden="true"><path fill-rule="evenodd" {d} /></svg>
  {/if}
  {#if word}
    <span><span class="a">bawk</span><span class="b">bit</span></span>
  {:else}
    <span class="sr-only">bawkbit</span>
  {/if}
</span>

<style>
  .logo {
    display: inline-flex;
    align-items: center;
    gap: 0.45em;
    font-weight: 600;
    letter-spacing: -0.01em;
    white-space: nowrap;
  }
  .mark {
    height: 1.25em;
    width: auto;
    fill: var(--text-strong);
    flex: none;
  }
  .a {
    color: var(--text-strong);
  }
  .b {
    color: var(--text-weak);
  }
</style>
