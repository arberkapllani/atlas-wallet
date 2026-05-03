<script lang="ts">
  import { wallet } from '$lib/stores/wallet';
  import { prices, COINGECKO_IDS } from '$lib/stores/prices';
  import { formatAmount, type Amount } from '$lib/api';
  import Button from '$lib/ui/Button.svelte';
  import Card from '$lib/ui/Card.svelte';

  $: total = (() => {
    let usd = 0;
    for (const c of $wallet.chains) {
      const bal = $wallet.balances[c.id];
      const cgId = COINGECKO_IDS[c.id];
      const price = cgId ? $prices[cgId]?.usd : undefined;
      if (bal && price) {
        const decimal = Number(bal.value) / 10 ** bal.asset.decimals;
        usd += decimal * price;
      }
    }
    return usd;
  })();

  function balanceUsd(b: Amount | null | undefined, chainId: string): number | null {
    if (!b) return null;
    const cg = COINGECKO_IDS[chainId];
    const p = cg ? $prices[cg]?.usd : undefined;
    if (!p) return null;
    return (Number(b.value) / 10 ** b.asset.decimals) * p;
  }
</script>

<div class="p-8 space-y-6 max-w-5xl">
  <header class="flex items-baseline justify-between">
    <div>
      <h1 class="text-2xl font-bold">Portfolio</h1>
      <p class="text-fg-muted text-sm mt-1">Your assets across every supported chain.</p>
    </div>
    <Button variant="secondary" on:click={() => wallet.refreshBalances()} loading={$wallet.loading}>
      Refresh
    </Button>
  </header>

  <Card>
    <div class="text-fg-subtle text-xs uppercase tracking-wider">Total balance</div>
    <div class="mt-2 text-4xl font-bold tracking-tight">
      ${total.toLocaleString(undefined, { maximumFractionDigits: 2 })}
    </div>
  </Card>

  <Card title="Assets">
    <div class="-mx-6 -mb-6">
      {#each $wallet.chains as chain}
        {@const bal = $wallet.balances[chain.id]}
        {@const usd = balanceUsd(bal, chain.id)}
        {@const cg = COINGECKO_IDS[chain.id]}
        {@const change = cg ? $prices[cg]?.usd_24h_change : undefined}
        <div class="flex items-center justify-between px-6 py-4 border-t border-border-subtle">
          <div>
            <div class="font-semibold">{chain.display_name}</div>
            <div class="text-xs text-fg-muted">{chain.symbol}</div>
          </div>
          <div class="text-right">
            <div class="font-mono text-sm">
              {bal ? formatAmount(bal) : '—'}
            </div>
            <div class="text-xs text-fg-muted flex items-center gap-2 justify-end">
              {#if usd != null}
                ${usd.toLocaleString(undefined, { maximumFractionDigits: 2 })}
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
</div>
