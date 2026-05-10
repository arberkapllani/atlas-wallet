<script lang="ts">
  import Card from '$lib/ui/Card.svelte';
  import Button from '$lib/ui/Button.svelte';
  import { api } from '$lib/api';

  let stage: 'idle' | 'verify' | 'revealed' = 'idle';
  let password = '';
  let phrase = '';
  let busy = false;
  let error = '';
  let acknowledged = false;
  let copied = false;

  async function reveal() {
    if (!password) return;
    busy = true;
    error = '';
    try {
      phrase = await api.revealPhrase(password);
      stage = 'revealed';
      password = '';
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  function hide() {
    phrase = '';
    password = '';
    error = '';
    acknowledged = false;
    copied = false;
    stage = 'idle';
  }

  async function copy() {
    if (!phrase) return;
    try {
      await navigator.clipboard.writeText(phrase);
      copied = true;
      setTimeout(() => (copied = false), 2000);
    } catch {
      // ignore — some environments block clipboard
    }
  }

  $: words = phrase ? phrase.split(/\s+/) : [];
</script>

<Card
  title="Backup recovery phrase"
  subtitle="View your 12 or 24-word recovery phrase. Anyone with this phrase can spend your funds."
>
  {#if stage === 'idle'}
    <div class="space-y-4">
      <div class="rounded-xl border border-accent/40 bg-accent/5 p-4 text-sm space-y-2">
        <div class="font-semibold text-accent-400">Before you continue</div>
        <ul class="list-disc list-inside text-fg-muted space-y-1">
          <li>Make sure no one is watching your screen.</li>
          <li>Never type or paste this phrase into any website or app.</li>
          <li>Write it on paper or store it on a metal backup — not in cloud notes.</li>
          <li>GreenWallet support will <strong>never</strong> ask for these words.</li>
        </ul>
      </div>
      <Button variant="secondary" on:click={() => (stage = 'verify')}>
        I understand — show recovery phrase
      </Button>
    </div>
  {:else if stage === 'verify'}
    <form on:submit|preventDefault={reveal} class="space-y-3">
      <label class="block text-sm">
        <span class="text-fg-muted">Confirm your password</span>
        <input
          type="password"
          bind:value={password}
          autocomplete="current-password"
          class="mt-1 w-full rounded-lg bg-bg-elevated border border-border px-3 py-2 font-mono focus:outline-none focus:border-accent"
        />
      </label>
      {#if error}
        <p class="text-danger text-sm">{error}</p>
      {/if}
      <div class="flex gap-2">
        <Button type="submit" loading={busy}>Reveal phrase</Button>
        <Button variant="secondary" on:click={hide}>Cancel</Button>
      </div>
    </form>
  {:else}
    <div class="space-y-4">
      <div
        class="rounded-xl border border-brand-500/40 bg-brand-500/5 p-4 grid grid-cols-2 sm:grid-cols-3 gap-2 font-mono text-sm select-text"
      >
        {#each words as w, i}
          <div class="flex items-center gap-2">
            <span class="text-fg-subtle text-xs w-5 text-right">{i + 1}.</span>
            <span class="font-semibold">{w}</span>
          </div>
        {/each}
      </div>

      <label class="flex items-start gap-2 text-sm text-fg-muted">
        <input type="checkbox" bind:checked={acknowledged} class="mt-1" />
        <span>I have written these words down and stored them somewhere safe.</span>
      </label>

      <div class="flex flex-wrap gap-2">
        <Button variant="secondary" on:click={copy}>
          {copied ? 'Copied!' : 'Copy to clipboard'}
        </Button>
        <Button on:click={hide} disabled={!acknowledged}>Done — hide phrase</Button>
      </div>
    </div>
  {/if}
</Card>
