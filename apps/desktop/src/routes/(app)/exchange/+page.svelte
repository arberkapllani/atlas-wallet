<script lang="ts">
  import Card from '$lib/ui/Card.svelte';
</script>

<div class="p-8 max-w-3xl space-y-6">
  <header>
    <h1 class="text-2xl font-bold">Exchange</h1>
    <p class="text-fg-muted text-sm mt-1">
      Swap assets without leaving Atlas. Powered by on-chain aggregators —
      no KYC, no custody, no middleman.
    </p>
  </header>

  <Card>
    <div class="text-center py-10">
      <div
        class="mx-auto h-14 w-14 rounded-2xl bg-brand-gradient flex items-center justify-center text-white font-bold shadow-glow mb-4"
      >
        ⇄
      </div>
      <h2 class="text-lg font-semibold">Coming in Phase 5</h2>
      <p class="text-sm text-fg-muted mt-2 max-w-md mx-auto">
        EVM swaps via the 1inch v6 aggregator (USDT ⇄ USDC ⇄ ETH ⇄ WBTC) ship
        first, followed by cross-chain swaps via Thorchain.
      </p>
    </div>
  </Card>
</div>
