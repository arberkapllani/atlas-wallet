<script lang="ts">
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import { page } from '$app/stores';
  import { wallet } from '$lib/stores/wallet';
  import { prices, startPriceFeed, stopPriceFeed, COINGECKO_IDS } from '$lib/stores/prices';
  import {
    fiatCurrency,
    loadFiatCurrency,
    setFiatCurrency,
    formatFiatStore
  } from '$lib/stores/currency';
  import type { FiatCurrency } from '$lib/api';
  import { api } from '$lib/api';
  import Logo from '$lib/ui/Logo.svelte';
  import ThemeToggle from '$lib/ui/ThemeToggle.svelte';

  let autoLockMinutes = 5;
  let idleTimer: ReturnType<typeof setTimeout> | null = null;

  function clearIdle() {
    if (idleTimer) {
      clearTimeout(idleTimer);
      idleTimer = null;
    }
  }

  function armIdle() {
    clearIdle();
    if (autoLockMinutes <= 0) return;
    const ms = autoLockMinutes * 60 * 1000;
    idleTimer = setTimeout(async () => {
      stopPriceFeed();
      await wallet.lock();
      await goto('/unlock');
    }, ms);
  }

  function onActivity() {
    armIdle();
  }

  onMount(() => {
    void loadFiatCurrency();
    void api
      .getAutoLockMinutes()
      .then((m) => {
        autoLockMinutes = m;
        armIdle();
      })
      .catch(() => {
        /* keep default */
      });

    const events: (keyof WindowEventMap)[] = [
      'mousemove',
      'mousedown',
      'keydown',
      'wheel',
      'touchstart'
    ];
    for (const e of events) {
      window.addEventListener(e, onActivity, { passive: true });
    }

    const sub = wallet.subscribe(async ($w) => {
      if (!$w.statusLoaded) return;
      if ($w.initialized && !$w.unlocked) await goto('/unlock');
      if (!$w.initialized) await goto('/onboarding');
      if ($w.chains.length > 0) {
        void startPriceFeed($w.chains.map((c) => c.id));
      }
    });

    return () => {
      clearIdle();
      for (const e of events) window.removeEventListener(e, onActivity);
      sub();
    };
  });

  /** Computed total fiat balance across every chain. */
  $: totalFiat = (() => {
    let v = 0;
    for (const c of $wallet.chains) {
      const bal = $wallet.balances[c.id];
      const cgId = COINGECKO_IDS[c.id];
      const price = cgId ? $prices[cgId]?.price : undefined;
      if (bal && price) {
        v += (Number(bal.value) / 10 ** bal.asset.decimals) * price;
      }
    }
    return v;
  })();

  async function pickCurrency(e: Event) {
    const next = (e.target as HTMLSelectElement).value as FiatCurrency;
    try {
      await setFiatCurrency(next);
    } catch (err) {
      console.warn('failed to set currency', err);
    }
  }

  function isActive(path: string): boolean {
    return $page.url.pathname.startsWith(path);
  }

  function navClass(path: string): string {
    return isActive(path)
      ? 'bg-brand-600/10 text-fg border-l-2 border-accent'
      : 'text-fg-muted hover:text-fg hover:bg-bg-elevated border-l-2 border-transparent';
  }

  function mobileNavClass(path: string): string {
    return isActive(path)
      ? 'text-accent'
      : 'text-fg-muted hover:text-fg';
  }

  async function lock() {
    stopPriceFeed();
    await wallet.lock();
    void goto('/unlock');
  }

  async function syncNow() {
    void wallet.refreshBalances();
  }

  /** Nav definition — icons inlined to avoid extra deps. */
  const navItems = [
    {
      href: '/portfolio',
      label: 'Portfolio',
      icon: 'M3 3v18h18M7 14l4-4 4 4 5-7'
    },
    {
      href: '/send',
      label: 'Send',
      icon: 'M5 12h14M13 6l6 6-6 6'
    },
    {
      href: '/receive',
      label: 'Receive',
      icon: 'M19 12H5M11 18l-6-6 6-6'
    },
    {
      href: '/exchange',
      label: 'Exchange',
      icon: 'M7 16V4m0 0L3 8m4-4l4 4m6 4v12m0 0l4-4m-4 4l-4-4'
    },
    {
      href: '/history',
      label: 'History',
      icon: 'M3 12a9 9 0 109-9 9 9 0 00-9 9zm9-5v5l3 2'
    },
    {
      href: '/nfts',
      label: 'NFTs',
      icon: 'M4 6h16v12H4zM4 6l8 6 8-6'
    },
    {
      href: '/wallets',
      label: 'Wallets',
      icon: 'M21 7H6a3 3 0 010-6h13v6zm0 0v10a3 3 0 01-3 3H6a3 3 0 01-3-3V4'
    },
    {
      href: '/settings',
      label: 'Settings',
      icon: 'M12 15a3 3 0 100-6 3 3 0 000 6zm7.4-3a7.4 7.4 0 00-.1-1.3l2.1-1.6-2-3.5-2.5 1a7.4 7.4 0 00-2.2-1.3L14.3 2h-4l-.4 2.6a7.4 7.4 0 00-2.2 1.3l-2.5-1-2 3.5 2.1 1.6a7.4 7.4 0 000 2.6l-2.1 1.6 2 3.5 2.5-1a7.4 7.4 0 002.2 1.3l.4 2.6h4l.4-2.6a7.4 7.4 0 002.2-1.3l2.5 1 2-3.5-2.1-1.6c.07-.43.1-.86.1-1.3z'
    }
  ];
</script>

<div
  class="grid min-h-screen bg-bg bg-mesh-dark dark:bg-mesh-dark grid-cols-1 md:grid-cols-[72px_1fr] lg:grid-cols-[240px_1fr]"
>
  <!-- Sidebar (hidden on mobile, icons-only md, full lg) -->
  <aside
    class="hidden md:flex border-r border-border-subtle bg-bg-subtle/70 glass flex-col"
  >
    <div class="px-4 lg:px-6 py-5 border-b border-border-subtle flex items-center gap-3 tauri-drag">
      <div class="lg:hidden tauri-no-drag">
        <Logo size="sm" showWordmark={false} />
      </div>
      <div class="hidden lg:block tauri-no-drag">
        <Logo size="md" />
      </div>
    </div>

    <nav class="flex-1 py-4 space-y-0.5">
      {#each navItems as item}
        <a
          href={item.href}
          class="flex items-center gap-3 px-4 lg:px-6 py-2.5 text-sm font-medium transition {navClass(
            item.href
          )}"
          title={item.label}
        >
          <svg
            xmlns="http://www.w3.org/2000/svg"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="1.8"
            stroke-linecap="round"
            stroke-linejoin="round"
            class="h-5 w-5 flex-none"
            aria-hidden="true"
          >
            <path d={item.icon} />
          </svg>
          <span class="hidden lg:inline">{item.label}</span>
        </a>
      {/each}
    </nav>

    <div class="p-3 lg:p-4 border-t border-border-subtle">
      <button
        on:click={lock}
        class="w-full text-sm text-fg-muted hover:text-fg px-3 py-2 rounded-lg hover:bg-bg-elevated transition flex items-center gap-2 justify-center lg:justify-start"
        title="Lock wallet"
      >
        <svg
          xmlns="http://www.w3.org/2000/svg"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="1.8"
          stroke-linecap="round"
          stroke-linejoin="round"
          class="h-4 w-4"
          aria-hidden="true"
        >
          <rect x="3" y="11" width="18" height="11" rx="2" />
          <path d="M7 11V7a5 5 0 0110 0v4" />
        </svg>
        <span class="hidden lg:inline">Lock wallet</span>
      </button>
    </div>
  </aside>

  <!-- Main column with top bar + content -->
  <section class="flex flex-col min-h-screen">
    <header
      class="h-14 flex items-center justify-between px-4 sm:px-6 border-b border-border-subtle bg-bg-subtle/60 glass tauri-drag"
    >
      <div class="flex items-center gap-3 tauri-no-drag">
        <!-- Mobile logo -->
        <div class="md:hidden">
          <Logo size="sm" showWordmark={false} />
        </div>
        <div class="flex items-baseline gap-3">
          <span class="hidden sm:inline text-xs uppercase tracking-[0.18em] text-fg-subtle">
            Total
          </span>
          <span class="text-base sm:text-lg font-display font-bold tracking-tight gradient-text">
            {$formatFiatStore(totalFiat)}
          </span>
        </div>
      </div>
      <div class="flex items-center gap-1 sm:gap-2 tauri-no-drag">
        <label class="sr-only" for="currency-picker">Display currency</label>
        <select
          id="currency-picker"
          value={$fiatCurrency}
          on:change={pickCurrency}
          class="h-9 px-2 rounded-lg text-xs bg-bg-elevated border border-border-subtle text-fg-muted hover:text-fg focus:outline-none focus:ring-2 focus:ring-accent/40"
          title="Display currency"
        >
          <option value="usd">USD $</option>
          <option value="eur">EUR €</option>
          <option value="gbp">GBP £</option>
        </select>
        <button
          on:click={syncNow}
          class="h-9 px-3 rounded-lg text-sm text-fg-muted hover:text-fg hover:bg-bg-elevated transition flex items-center gap-2 disabled:opacity-50"
          disabled={$wallet.loading}
          title="Refresh balances"
        >
          <svg
            xmlns="http://www.w3.org/2000/svg"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="1.8"
            stroke-linecap="round"
            stroke-linejoin="round"
            class="h-4 w-4 {$wallet.loading ? 'animate-spin' : ''}"
            aria-hidden="true"
          >
            <path d="M21 12a9 9 0 11-3-6.7L21 8" />
            <path d="M21 3v5h-5" />
          </svg>
          <span class="hidden sm:inline">{$wallet.loading ? 'Syncing…' : 'Sync'}</span>
        </button>
        <ThemeToggle />
      </div>
    </header>

    <div class="flex-1 overflow-auto pb-16 md:pb-0">
      <slot />
    </div>

    <!-- Mobile bottom navigation -->
    <nav
      class="md:hidden fixed bottom-0 inset-x-0 h-16 bg-bg-subtle/90 glass border-t border-border-subtle flex items-stretch justify-around z-40"
    >
      {#each navItems.slice(0, 5) as item}
        <a
          href={item.href}
          class="flex-1 flex flex-col items-center justify-center gap-1 text-[10px] font-medium {mobileNavClass(
            item.href
          )}"
        >
          <svg
            xmlns="http://www.w3.org/2000/svg"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="1.8"
            stroke-linecap="round"
            stroke-linejoin="round"
            class="h-5 w-5"
            aria-hidden="true"
          >
            <path d={item.icon} />
          </svg>
          {item.label}
        </a>
      {/each}
    </nav>
  </section>
</div>
