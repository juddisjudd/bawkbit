<script lang="ts">
  import { app } from '$lib/state.svelte'
  import Logo from './Logo.svelte'

  let username = $state('')
  let password = $state('')
  let error = $state('')
  let busy = $state(false)

  const noPassword = $derived(app.session !== null && !app.session.has_password)

  async function submit(event: SubmitEvent): Promise<void> {
    event.preventDefault()
    busy = true
    error = ''
    try {
      await app.login(username, password)
    } catch (err) {
      error = (err as Error).message
      password = ''
    } finally {
      busy = false
    }
  }
</script>

<main class="login">
  <form onsubmit={submit} aria-label="log in">
    <div class="brand"><Logo size={22} /></div>
    {#if noPassword}
      <p class="muted center">set a web UI password in bawkbit's settings, then come back.</p>
    {:else}
      <input
        class="input"
        name="username"
        autocomplete="username"
        placeholder="name"
        aria-label="name"
        spellcheck="false"
        autocapitalize="none"
        required
        bind:value={username}
      />
      <input
        class="input"
        name="password"
        type="password"
        autocomplete="current-password"
        placeholder="password"
        aria-label="password"
        required
        bind:value={password}
      />
      <p class="error" role="alert">{error}</p>
      <button class="btn strong" type="submit" disabled={busy || !username || !password}>log in</button>
    {/if}
  </form>
</main>

<style>
  .login {
    display: grid;
    place-items: center;
    height: 100%;
    padding: 24px;
  }
  form {
    display: flex;
    flex-direction: column;
    gap: 10px;
    width: min(300px, 100%);
  }
  .brand {
    display: flex;
    justify-content: center;
    margin-bottom: 18px;
  }
  .center {
    text-align: center;
  }
  .error {
    min-height: 1.5em;
    color: var(--danger);
    font-size: 12px;
  }
  .btn {
    height: 32px;
  }
</style>
