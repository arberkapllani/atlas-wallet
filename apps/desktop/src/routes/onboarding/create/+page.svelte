<script lang="ts">
  import { goto } from '$app/navigation';
  import Button from '$lib/ui/Button.svelte';
  import Card from '$lib/ui/Card.svelte';
  import Input from '$lib/ui/Input.svelte';
  import { wallet } from '$lib/stores/wallet';

  let step: 'password' | 'reveal' | 'confirm' = 'password';
  let password = '';
  let confirmPwd = '';
  let wordCount: 12 | 24 = 12;
  let phrase = '';
  let acknowledged = false;
  let busy = false;
  let error = '';

  async function generate() {
    if (password.length < 8) { error = 'Password must be at least 8 characters.'; return; }
    if (password !== confirmPwd) { error = 'Passwords do not match.'; return; }
    error = '';
    busy = true;
    try {
      phrase = await wallet.createNew(password, wordCount);
      step = 'reveal';
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  function proceed() {
    if (!acknowledged) return;
    void goto('/portfolio');
  }
</script>

<div class="min-h-screen grid place-items-center px-6 py-10">
  <div class="w-full max-w-xl">
    {#if step === 'password'}
      <Card title="Create a wallet" subtitle="Pick a strong password to encrypt your keys at rest.">
        <div class="space-y-4">
          <Input label="Password" type="password" autocomplete="new-password" bind:value={password} />
          <Input label="Confirm password" type="password" autocomplete="new-password" bind:value={confirmPwd} />
          <div>
            <span class="text-sm text-fg-muted block mb-2">Recovery-phrase length</span>
            <div class="flex gap-2">
              <button
                class="flex-1 rounded-xl px-4 py-3 border transition {wordCount === 12 ? 'border-accent bg-accent/10' : 'border-border bg-bg-elevated'}"
                on:click={() => (wordCount = 12)}
              >
                <div class="font-semibold">12 words</div>
                <div class="text-xs text-fg-muted">Recommended</div>
              </button>
              <button
                class="flex-1 rounded-xl px-4 py-3 border transition {wordCount === 24 ? 'border-accent bg-accent/10' : 'border-border bg-bg-elevated'}"
                on:click={() => (wordCount = 24)}
              >
                <div class="font-semibold">24 words</div>
                <div class="text-xs text-fg-muted">Maximum entropy</div>
              </button>
            </div>
          </div>
          {#if error}<p class="text-sm text-danger">{error}</p>{/if}
          <div class="flex gap-3">
            <Button variant="secondary" on:click={() => goto('/onboarding')}>Back</Button>
            <Button fullWidth loading={busy} on:click={generate}>Generate phrase</Button>
          </div>
        </div>
      </Card>
    {:else if step === 'reveal'}
      <Card title="Your recovery phrase" subtitle="Write these words down in order. Anyone with this phrase can spend your funds.">
        <div class="space-y-5">
          <div class="grid grid-cols-3 gap-2 font-mono">
            {#each phrase.split(' ') as word, i}
              <div class="rounded-lg border border-border-subtle bg-bg-elevated px-3 py-2 text-sm">
                <span class="text-fg-subtle mr-2">{i + 1}.</span>{word}
              </div>
            {/each}
          </div>
          <label class="flex items-center gap-3 text-sm text-fg-muted">
            <input type="checkbox" class="h-4 w-4 accent-accent" bind:checked={acknowledged} />
            I have written down my recovery phrase and stored it safely.
          </label>
          <Button fullWidth disabled={!acknowledged} on:click={proceed}>Continue</Button>
        </div>
      </Card>
    {/if}
  </div>
</div>
