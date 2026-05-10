<script lang="ts">
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import Button from '$lib/ui/Button.svelte';
  import Card from '$lib/ui/Card.svelte';
  import Input from '$lib/ui/Input.svelte';
  import Logo from '$lib/ui/Logo.svelte';
  import ThemeToggle from '$lib/ui/ThemeToggle.svelte';
  import { wallet } from '$lib/stores/wallet';
  import { api } from '$lib/api';

  let password = '';
  let busy = false;
  let error = '';
  let antiPhishingPhrase: string | null = null;

  onMount(() => {
    void api
      .getAntiPhishingPhrase()
      .then((p) => (antiPhishingPhrase = p))
      .catch(() => {
        /* feature optional */
      });
  });

  async function unlock() {
    error = '';
    busy = true;
    try {
      await wallet.unlock(password);
      void goto('/portfolio');
    } catch (e) {
      error = 'Invalid password.';
      console.error(e);
    } finally {
      busy = false;
    }
  }
</script>

<div class="min-h-screen grid place-items-center px-6 bg-bg bg-mesh-dark relative">
  <div class="absolute top-4 right-4">
    <ThemeToggle />
  </div>
  <div class="w-full max-w-md animate-fade-in-up">
    <div class="flex justify-center mb-6">
      <Logo size="lg" tagline="Welcome back" />
    </div>
    <Card title="Unlock your wallet" subtitle="Enter your password to continue." glow>
      {#if antiPhishingPhrase}
        <div
          class="mb-4 rounded-xl border border-accent/40 bg-accent/5 px-4 py-3 text-sm"
          data-testid="anti-phishing-phrase"
        >
          <span class="block text-[10px] uppercase tracking-[0.18em] text-fg-muted mb-1">
            Anti-phishing phrase
          </span>
          <span class="font-mono text-base text-fg">{antiPhishingPhrase}</span>
          <span class="mt-2 block text-xs text-fg-muted">
            If this doesn't match what you set, this is not GreenWallet. Quit immediately.
          </span>
        </div>
      {/if}
      <form on:submit|preventDefault={unlock} class="space-y-4">
        <Input
          label="Password"
          type="password"
          bind:value={password}
          autocomplete="current-password"
        />
        {#if error}<p class="text-sm text-danger">{error}</p>{/if}
        <Button type="submit" fullWidth loading={busy}>Unlock</Button>
      </form>
    </Card>
  </div>
</div>
