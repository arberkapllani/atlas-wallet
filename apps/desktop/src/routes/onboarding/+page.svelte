<script lang="ts">
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import { wallet } from '$lib/stores/wallet';
  import Card from '$lib/ui/Card.svelte';
  import Logo from '$lib/ui/Logo.svelte';
  import ThemeToggle from '$lib/ui/ThemeToggle.svelte';

  let refreshing = false;

  // If a vault already exists on disk, never show onboarding — bounce to /unlock.
  onMount(() => {
    return wallet.subscribe(($w) => {
      if ($w.statusLoaded && $w.initialized) void goto('/unlock');
    });
  });

  async function recheck() {
    refreshing = true;
    try {
      await wallet.refreshStatus();
    } finally {
      refreshing = false;
    }
  }
</script>

<div class="min-h-screen grid place-items-center px-6 bg-bg bg-mesh-dark relative">
  <div class="absolute top-4 right-4 flex items-center gap-2">
    <button
      type="button"
      on:click={recheck}
      disabled={refreshing}
      title="Re-check for existing wallet"
      aria-label="Re-check for existing wallet"
      class="h-9 w-9 grid place-items-center rounded-full bg-bg-elevated hover:bg-border border border-border transition disabled:opacity-50"
    >
      <svg
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="1.8"
        class="h-4 w-4 {refreshing ? 'animate-spin' : ''}"
      >
        <path stroke-linecap="round" stroke-linejoin="round" d="M21 12a9 9 0 1 1-3-6.7M21 4v5h-5" />
      </svg>
    </button>
    <ThemeToggle />
  </div>
  <div class="w-full max-w-xl space-y-8 animate-fade-in-up">
    <header class="text-center space-y-4">
      <div class="flex justify-center">
        <Logo size="lg" showWordmark={false} />
      </div>
      <h1 class="text-4xl sm:text-5xl font-display font-bold tracking-tight">
        Welcome to <span class="gradient-text">GreenWallet</span>
      </h1>
      <p class="text-fg-muted text-lg max-w-md mx-auto">
        A sovereign multi-chain wallet. Your keys, your coins, your sovereignty.
      </p>
    </header>

    <Card>
      <div class="space-y-3">
        <a href="/onboarding/create" class="block group">
          <div
            class="rounded-xl border border-border bg-bg-elevated p-5 hover:border-brand-500 hover:shadow-glow transition-all cursor-pointer"
          >
            <div class="flex items-start gap-4">
              <div
                class="h-10 w-10 rounded-lg bg-brand-600/10 text-brand-500 grid place-items-center flex-none"
              >
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" class="h-5 w-5">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M12 4v16m-8-8h16" />
                </svg>
              </div>
              <div>
                <h3 class="font-semibold mb-1 group-hover:text-brand-500 transition">
                  Create a new wallet
                </h3>
                <p class="text-sm text-fg-muted">
                  Generate a fresh 12-word recovery phrase. Your keys never leave this device.
                </p>
              </div>
            </div>
          </div>
        </a>

        <a href="/onboarding/import" class="block group">
          <div
            class="rounded-xl border border-border bg-bg-elevated p-5 hover:border-accent hover:shadow-accentGlow transition-all cursor-pointer"
          >
            <div class="flex items-start gap-4">
              <div
                class="h-10 w-10 rounded-lg bg-accent/10 text-accent grid place-items-center flex-none"
              >
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" class="h-5 w-5">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M4 16l4-4-4-4m4 4h12" />
                </svg>
              </div>
              <div>
                <h3 class="font-semibold mb-1 group-hover:text-accent transition">
                  Import an existing wallet
                </h3>
                <p class="text-sm text-fg-muted">
                  Recover from a 12 or 24-word BIP-39 phrase. Optional 25th-word passphrase supported.
                </p>
              </div>
            </div>
          </div>
        </a>
      </div>
    </Card>

    <p class="text-xs text-fg-subtle text-center uppercase tracking-[0.18em]">
      Open source · Non-custodial · No accounts · No KYC
    </p>
  </div>
</div>
