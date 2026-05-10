<script lang="ts">
  import Card from '$lib/ui/Card.svelte';
  import Button from '$lib/ui/Button.svelte';
  import { api, errorMessage, type GalleryFilter, type GalleryView, type OwnedNft } from '$lib/api';

  let raw = '';
  let hideSpam = true;
  let minVal = 0;
  let chains = '';
  let result: GalleryView | null = null;
  let error = '';
  let busy = false;

  function loadDemo() {
    const demo: OwnedNft[] = [
      {
        chain_id: 'eth',
        contract: '0xbc4ca0eda7647a8ab7c2061c2e118a18a936f13d',
        token_id: '8817',
        name: 'BAYC #8817',
        collection: 'Bored Ape Yacht Club',
        image: 'ipfs://bayc/8817.png',
        floor_usd: 30000.0
      } as OwnedNft,
      {
        chain_id: 'eth',
        contract: '0xbc4ca0eda7647a8ab7c2061c2e118a18a936f13d',
        token_id: '101',
        name: 'BAYC #101',
        collection: 'Bored Ape Yacht Club',
        image: 'ipfs://bayc/101.png',
        floor_usd: 30000.0
      } as OwnedNft,
      {
        chain_id: 'polygon',
        contract: '0xbcdefabc0000000000000000000000000000beef',
        token_id: '1',
        name: 'Free $1000 USDT! Visit airdrop.example',
        collection: 'AIRDROP CLAIM',
        image: null,
        floor_usd: null
      } as OwnedNft,
      {
        chain_id: 'eth',
        contract: '0xc0ffee0000000000000000000000000000c0ffee',
        token_id: '42',
        name: 'CryptoCoffee #42',
        collection: 'CryptoCoffee',
        image: null,
        floor_usd: null
      } as OwnedNft
    ];
    raw = JSON.stringify(demo, null, 2);
    result = null;
    error = '';
  }

  async function build() {
    if (!raw.trim()) return;
    busy = true;
    error = '';
    result = null;
    try {
      const items = JSON.parse(raw) as OwnedNft[];
      const filter: GalleryFilter = {
        hide_spam: hideSpam,
        min_collection_value_usd: Number(minVal) || 0,
        chains: chains
          .split(',')
          .map((s) => s.trim().toLowerCase())
          .filter(Boolean)
      };
      result = await api.nftGalleryView(items, filter);
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  function fmtUsd(n: number): string {
    return n.toLocaleString(undefined, {
      style: 'currency',
      currency: 'USD',
      maximumFractionDigits: 0
    });
  }
</script>

<Card
  title="NFT gallery aggregator"
  subtitle="Pure offline aggregation: paste a list of OwnedNft objects (or load demo) and atlas-nft-gallery groups them into collections, applies the spam filter and sorts by estimated USD value."
>
  <div class="space-y-3 text-sm">
    {#if error}
      <p class="text-rose-400 text-xs">{error}</p>
    {/if}

    <div class="flex items-center gap-2 flex-wrap">
      <Button variant="secondary" on:click={loadDemo} disabled={busy}>Load demo data</Button>
      <Button variant="primary" on:click={build} disabled={busy || !raw.trim()}>
        {busy ? 'Building…' : 'Build gallery'}
      </Button>
    </div>

    <textarea
      bind:value={raw}
      rows="4"
      placeholder={'[{"chain_id":"eth","contract":"0x...","token_id":"1",...}]'}
      class="w-full px-3 py-2 rounded-md bg-bg-elevated border border-border-subtle text-fg font-mono text-[11px]"
    ></textarea>

    <div class="grid grid-cols-3 gap-2">
      <label class="text-xs text-fg-muted flex items-center gap-2 col-span-1">
        <input type="checkbox" bind:checked={hideSpam} />
        Hide spam
      </label>
      <label class="text-xs text-fg-muted block col-span-1">
        Min value (USD)
        <input
          type="number"
          step="any"
          bind:value={minVal}
          class="mt-1 w-full h-9 px-2 rounded-md bg-bg-elevated border border-border-subtle text-fg text-xs font-mono"
        />
      </label>
      <label class="text-xs text-fg-muted block col-span-1">
        Chains (comma-sep)
        <input
          type="text"
          bind:value={chains}
          placeholder="eth,polygon"
          class="mt-1 w-full h-9 px-2 rounded-md bg-bg-elevated border border-border-subtle text-fg text-xs font-mono"
        />
      </label>
    </div>

    {#if result}
      <div class="rounded-lg border border-border-subtle p-3 space-y-2 text-xs">
        <div class="flex gap-4">
          <span><span class="text-fg-muted">Collections:</span> {result.total_collections}</span>
          <span><span class="text-fg-muted">Items:</span> {result.total_items}</span>
          <span
            ><span class="text-fg-muted">Total est. value:</span>
            <span class="font-mono">{fmtUsd(result.total_value_usd)}</span>
          </span>
        </div>

        {#if result.collections.length > 0}
          <ul class="border border-border-subtle rounded-md divide-y divide-border-subtle">
            {#each result.collections as c}
              <li class="px-3 py-2 flex items-center gap-3">
                <span class="font-medium">{c.name}</span>
                <span class="text-fg-subtle text-[10px] uppercase">{c.chain_id}</span>
                <span class="text-fg-muted text-[10px] font-mono">{c.item_count} items</span>
                {#if c.unpriced_count > 0}
                  <span class="text-amber-400 text-[10px]">({c.unpriced_count} unpriced)</span>
                {/if}
                <span class="ml-auto font-mono text-emerald-400">
                  {fmtUsd(c.estimated_value_usd)}
                </span>
              </li>
            {/each}
          </ul>
        {/if}

        {#if result.hidden_items.length > 0}
          <details>
            <summary class="cursor-pointer text-amber-400">
              Hidden / spam items ({result.hidden_items.length})
            </summary>
            <ul class="mt-1 list-disc list-inside text-fg-muted">
              {#each result.hidden_items as h}
                <li class="font-mono text-[10px]">
                  {h.chain_id}
                  {h.contract.slice(0, 10)}… #{h.token_id}
                  {h.name ? `— ${h.name}` : ''}
                </li>
              {/each}
            </ul>
          </details>
        {/if}
      </div>
    {/if}
  </div>
</Card>
