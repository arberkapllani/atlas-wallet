<script lang="ts">
  import { wallet } from '$lib/stores/wallet';
  import { prices, COINGECKO_IDS } from '$lib/stores/prices';
  import { formatFiatStore } from '$lib/stores/currency';
  import { formatAmount, type Amount } from '$lib/api';
  import Button from '$lib/ui/Button.svelte';
  import Card from '$lib/ui/Card.svelte';
  import DiversificationCard from '$lib/portfolio/DiversificationCard.svelte';

  $: total = (() => {
    let v = 0;
    for (const c of $wallet.chains) {
      const bal = $wallet.balances[c.id];
      const cgId = COINGECKO_IDS[c.id];
      const price = cgId ? $prices[cgId]?.price : undefined;
      if (bal && price) {
        const decimal = Number(bal.value) / 10 ** bal.asset.decimals;
        v += decimal * price;
      }
    }
    return v;
  })();

  function balanceFiat(b: Amount | null | undefined, chainId: string): number | null {
    if (!b) return null;
    const cg = COINGECKO_IDS[chainId];
    const p = cg ? $prices[cg]?.price : undefined;
    if (!p) return null;
    return (Number(b.value) / 10 ** b.asset.decimals) * p;
  }
</script>

<div class="p-4 sm:p-6 lg:p-8 space-y-6 max-w-6xl mx-auto animate-fade-in-up">
  <header class="flex flex-wrap items-baseline justify-between gap-3">
    <div>
      <h1 class="text-2xl sm:text-3xl font-display font-bold tracking-tight">Portfolio</h1>
      <p class="text-fg-muted text-sm mt-1">Your assets across every supported chain.</p>
    </div>
    <Button variant="secondary" on:click={() => wallet.refreshBalances()} loading={$wallet.loading}>
      Refresh
    </Button>
  </header>

  <!-- Hero balance card -->
  <Card glow>
    <div class="flex flex-col sm:flex-row sm:items-end sm:justify-between gap-4">
      <div>
        <div class="text-fg-subtle text-[10px] uppercase tracking-[0.18em]">Total balance</div>
        <div class="mt-2 text-4xl sm:text-5xl font-display font-bold tracking-tight gradient-text">
          {$formatFiatStore(total)}
        </div>
      </div>
      <div class="flex items-center gap-2">
        <a
          href="/send"
          class="inline-flex h-10 px-4 items-center gap-2 rounded-xl bg-brand-600 hover:bg-brand-500 text-white font-semibold shadow-glow transition active:scale-[0.98]"
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" class="h-4 w-4">
            <path stroke-linecap="round" stroke-linejoin="round" d="M5 12h14M13 6l6 6-6 6" />
          </svg>
          Send
        </a>
        <a
          href="/receive"
          class="inline-flex h-10 px-4 items-center gap-2 rounded-xl bg-bg-elevated hover:bg-border text-fg border border-border font-semibold transition active:scale-[0.98]"
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" class="h-4 w-4">
            <path stroke-linecap="round" stroke-linejoin="round" d="M19 12H5M11 18l-6-6 6-6" />
          </svg>
          Receive
        </a>
        <a
          href="/exchange"
          class="inline-flex h-10 px-4 items-center gap-2 rounded-xl bg-accent hover:bg-accent-hover text-white font-semibold shadow-accentGlow transition active:scale-[0.98]"
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" class="h-4 w-4">
            <path stroke-linecap="round" stroke-linejoin="round" d="M7 16V4m0 0L3 8m4-4l4 4m6 4v12m0 0l4-4m-4 4l-4-4" />
          </svg>
          Swap
        </a>
      </div>
    </div>
  </Card>

  <Card title="Assets">
    <div class="-mx-6 -mb-6">
      {#each $wallet.chains as chain}
        {@const bal = $wallet.balances[chain.id]}
        {@const fiat = balanceFiat(bal, chain.id)}
        {@const cg = COINGECKO_IDS[chain.id]}
        {@const change = cg ? $prices[cg]?.change_24h : undefined}
        <div
          class="flex items-center justify-between px-6 py-4 border-t border-border-subtle hover:bg-bg-elevated/40 transition"
        >
          <div class="flex items-center gap-3">
            <div
              class="h-9 w-9 rounded-full bg-brand-gradient grid place-items-center text-white text-xs font-bold ring-1 ring-white/10"
            >
              {chain.symbol.slice(0, 2)}
            </div>
            <div>
              <div class="font-semibold">{chain.display_name}</div>
              <div class="text-xs text-fg-muted">{chain.symbol}</div>
            </div>
          </div>
          <div class="text-right">
            <div class="font-mono text-sm">
              {bal ? formatAmount(bal) : '—'}
            </div>
            <div class="text-xs text-fg-muted flex items-center gap-2 justify-end">
              {#if fiat != null}
                {$formatFiatStore(fiat)}
              {:else}
                <span>—</span>
              {/if}
              {#if change != null}
                <span class={change >= 0 ? 'text-success' : 'text-danger'}>
                  {change >= 0 ? '+' : ''}{change.toFixed(2)}%
                </span>
              {/if}
            </div>
          </div>
        </div>
      {/each}
    </div>
  </Card>

  <DiversificationCard />
</div>
