<script lang="ts">
  import { wallet } from '$lib/stores/wallet';
  import { prices, COINGECKO_IDS } from '$lib/stores/prices';
  import Card from '$lib/ui/Card.svelte';
  import {
    api,
    errorMessage,
    type DiversificationReport,
    type DiversificationBand,
    type Holding
  } from '$lib/api';

  let report: DiversificationReport | null = null;
  let error = '';

  // Recompute holdings from the wallet store + price store, then call the
  // backend's diversification crate. We deliberately re-derive on every
  // wallet/price change so the band stays in sync with the asset list.
  $: holdings = (() => {
    const out: Holding[] = [];
    for (const c of $wallet.chains) {
      const bal = $wallet.balances[c.id];
      const cgId = COINGECKO_IDS[c.id];
      const price = cgId ? $prices[cgId]?.price : undefined;
      if (!bal || !price) continue;
      const decimal = Number(bal.value) / 10 ** bal.asset.decimals;
      const usd = decimal * price;
      if (usd > 0) out.push({ symbol: bal.asset.symbol, usd_value: usd });
    }
    return out;
  })();

  $: void analyse(holdings);

  async function analyse(h: Holding[]) {
    error = '';
    if (h.length === 0) {
      report = null;
      return;
    }
    try {
      report = await api.diversificationAnalyse(h);
    } catch (e) {
      error = errorMessage(e);
      report = null;
    }
  }

  function bandTone(b: DiversificationBand): string {
    switch (b) {
      case 'Diversified':
        return 'text-success';
      case 'Balanced':
        return 'text-fg';
      case 'Concentrated':
        return 'text-amber-400';
      case 'HighlyConcentrated':
        return 'text-rose-400';
    }
  }

  function bandLabel(b: DiversificationBand): string {
    return b === 'HighlyConcentrated' ? 'Highly concentrated' : b;
  }

  function pct(x: number): string {
    return `${(x * 100).toFixed(1)}%`;
  }
</script>

<Card
  title="Diversification"
  subtitle="Concentration measured by Herfindahl–Hirschman index across priced assets."
>
  <div class="space-y-4 text-sm">
    {#if error}
      <p class="text-rose-400 text-xs">{error}</p>
    {:else if !report}
      <p class="text-fg-subtle text-xs">
        No priced assets yet. Wait for balances and prices to load, or fund a wallet.
      </p>
    {:else}
      <div class="flex flex-wrap items-center gap-x-6 gap-y-2">
        <div>
          <span class="text-fg-muted text-xs">Band</span>
          <p class="font-medium {bandTone(report.band)}">{bandLabel(report.band)}</p>
        </div>
        <div>
          <span class="text-fg-muted text-xs">Positions</span>
          <p class="font-mono">{report.position_count}</p>
        </div>
        <div>
          <span class="text-fg-muted text-xs">Largest share</span>
          <p class="font-mono">{pct(report.largest_share)}</p>
        </div>
        <div>
          <span class="text-fg-muted text-xs">HHI</span>
          <p class="font-mono">{report.hhi}</p>
        </div>
      </div>

      <div class="space-y-1.5">
        {#each report.positions as p}
          <div>
            <div class="flex items-baseline justify-between text-xs">
              <span class="font-medium">{p.symbol}</span>
              <span class="font-mono text-fg-muted">{pct(p.weight)}</span>
            </div>
            <div class="h-1.5 rounded bg-bg-elevated overflow-hidden">
              <div class="h-full bg-accent" style="width: {Math.max(p.weight * 100, 0.5)}%"></div>
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</Card>
