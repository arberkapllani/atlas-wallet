<script lang="ts">
  import { wallet } from '$lib/stores/wallet';
  import Card from '$lib/ui/Card.svelte';
  import QrCode from '$lib/ui/QrCode.svelte';

  let chainId = '';
  $: chain = $wallet.chains.find((c) => c.id === chainId);
  $: address = chainId ? $wallet.addresses[chainId] : undefined;

  $: if (!chainId && $wallet.chains.length > 0) chainId = $wallet.chains[0].id;

  async function copy() {
    if (!address) return;
    try {
      await navigator.clipboard.writeText(address);
    } catch {}
  }
</script>

<div class="p-8 max-w-2xl space-y-6">
  <header>
    <h1 class="text-2xl font-bold">Receive</h1>
    <p class="text-fg-muted text-sm mt-1">Share an address to receive funds.</p>
  </header>

  <Card>
    <div class="space-y-5">
      <div>
        <span class="text-sm text-fg-muted block mb-2">Chain</span>
        <select
          bind:value={chainId}
          class="w-full bg-bg-elevated border border-border rounded-xl px-3.5 py-2.5 text-sm focus:outline-none focus:border-accent"
        >
          {#each $wallet.chains as c}
            <option value={c.id}>{c.display_name} ({c.symbol})</option>
          {/each}
        </select>
      </div>

      {#if address}
        <div class="flex flex-col items-center gap-4 py-4">
          <QrCode value={address} size={220} />
          <div class="font-mono text-sm break-all text-center px-4 select-all">{address}</div>
          <button on:click={copy} class="text-xs text-accent hover:text-accent-hover">
            Copy address
          </button>
          {#if chain}
            <div class="text-xs text-fg-subtle text-center">
              Only send {chain.symbol} on the {chain.display_name} network to this address.
            </div>
          {/if}
        </div>
      {/if}
    </div>
  </Card>
</div>
