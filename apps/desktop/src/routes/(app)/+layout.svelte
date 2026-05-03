<script lang="ts">
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import { page } from '$app/stores';
  import { wallet } from '$lib/stores/wallet';
  import { startPriceFeed, stopPriceFeed } from '$lib/stores/prices';

  onMount(() => {
    return wallet.subscribe(async ($w) => {
      if ($w.initialized && !$w.unlocked) await goto('/unlock');
      if (!$w.initialized) await goto('/onboarding');
      if ($w.chains.length > 0) {
        void startPriceFeed($w.chains.map((c) => c.id));
      }
    });
  });

  function active(path: string): string {
    return $page.url.pathname.startsWith(path)
      ? 'bg-accent/10 text-fg border-l-2 border-accent'
      : 'text-fg-muted hover:text-fg hover:bg-bg-elevated';
  }

  async function lock() {
    stopPriceFeed();
    await wallet.lock();
    void goto('/unlock');
  }
</script>

<div class="grid grid-cols-[240px_1fr] min-h-screen">
  <aside class="border-r border-border-subtle bg-bg-subtle flex flex-col">
    <div class="px-6 py-5 border-b border-border-subtle">
      <h1 class="font-bold tracking-tight text-lg">Exodus 2</h1>
      <p class="text-xs text-fg-subtle">Sovereign wallet</p>
    </div>
    <nav class="flex-1 py-4 space-y-1">
      <a href="/portfolio" class="block px-6 py-2.5 text-sm transition {active('/portfolio')}">Portfolio</a>
      <a href="/send"      class="block px-6 py-2.5 text-sm transition {active('/send')}">Send</a>
      <a href="/receive"   class="block px-6 py-2.5 text-sm transition {active('/receive')}">Receive</a>
      <a href="/settings"  class="block px-6 py-2.5 text-sm transition {active('/settings')}">Settings</a>
    </nav>
    <div class="p-4 border-t border-border-subtle">
      <button
        on:click={lock}
        class="w-full text-sm text-fg-muted hover:text-fg px-3 py-2 rounded-lg hover:bg-bg-elevated transition text-left"
      >
        Lock wallet
      </button>
    </div>
  </aside>

  <section class="overflow-auto">
    <slot />
  </section>
</div>
