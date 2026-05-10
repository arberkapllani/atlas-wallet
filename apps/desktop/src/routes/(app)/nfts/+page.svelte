<script lang="ts">
  import { onMount } from 'svelte';
  import Card from '$lib/ui/Card.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Input from '$lib/ui/Input.svelte';
  import { wallet } from '$lib/stores/wallet';
  import { commands, type OwnedNft } from '$lib/bindings';

  let supported: string[] = [];
  let chainId = '';
  let address = '';
  let items: OwnedNft[] = [];
  let loading = false;
  let error = '';

  async function defaultAddressFor(chain: string): Promise<string> {
    try {
      const res = await commands.getAddress(chain);
      if (res.status === 'ok') return res.data;
    } catch {
      /* ignore */
    }
    return '';
  }

  async function load() {
    if (!chainId || !address) return;
    loading = true;
    error = '';
    try {
      const res = await commands.nftListOwned(chainId, address);
      if (res.status === 'ok') {
        items = res.data;
      } else {
        error = 'message' in res.error ? res.error.message : res.error.kind;
        items = [];
      }
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  async function onChainChange() {
    address = await defaultAddressFor(chainId);
    if (chainId && address) await load();
  }

  function fmtUsd(v: number | null): string {
    if (v == null) return '—';
    return v.toLocaleString(undefined, { style: 'currency', currency: 'USD' });
  }

  onMount(async () => {
    const r = await commands.nftSupportedChains();
    if (r.status === 'ok') {
      supported = r.data;
      // Pick the first chain the user actually has loaded.
      const myChainIds = new Set($wallet.chains.map((c) => c.id));
      chainId = supported.find((c) => myChainIds.has(c)) ?? supported[0] ?? '';
      if (chainId) {
        address = await defaultAddressFor(chainId);
        if (address) await load();
      }
    }
  });
</script>

<div class="px-8 py-6 space-y-6">
  <header class="flex items-center justify-between gap-4">
    <div>
      <h2 class="text-2xl font-bold tracking-tight">NFTs</h2>
      <p class="text-sm text-fg-muted">
        Read-only view of NFTs owned by the active profile. Floor prices via Reservoir.
      </p>
    </div>
    <Button on:click={load} {loading} disabled={!chainId || !address}>Refresh</Button>
  </header>

  <Card>
    <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
      <div>
        <label class="block text-xs uppercase tracking-wider text-fg-subtle mb-2" for="nft-chain">
          Chain
        </label>
        <select
          id="nft-chain"
          bind:value={chainId}
          on:change={onChainChange}
          class="w-full h-10 px-3 rounded-lg text-sm bg-bg-elevated border border-border-subtle"
        >
          {#each supported as c}
            <option value={c}>{c}</option>
          {/each}
        </select>
      </div>
      <Input label="Address" bind:value={address} autocomplete="off" />
    </div>
  </Card>

  {#if error}
    <div class="rounded-lg border border-danger/40 bg-danger/5 px-4 py-3 text-sm text-danger">
      {error}
    </div>
  {/if}

  {#if loading && !items.length}
    <p class="text-sm text-fg-muted">Loading…</p>
  {:else if !items.length}
    <p class="text-sm text-fg-muted">No NFTs found for this address on {chainId || '—'}.</p>
  {:else}
    <div class="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-4 gap-4">
      {#each items as nft}
        <Card>
          <div class="aspect-square rounded-lg overflow-hidden bg-bg-elevated mb-3">
            {#if nft.image}
              <img
                src={nft.image}
                alt={nft.name ?? `NFT ${nft.token_id}`}
                class="h-full w-full object-cover"
                loading="lazy"
              />
            {:else}
              <div class="h-full w-full grid place-items-center text-fg-subtle text-xs">
                No image
              </div>
            {/if}
          </div>
          <div class="text-sm font-semibold truncate" title={nft.name ?? nft.token_id}>
            {nft.name ?? `#${nft.token_id}`}
          </div>
          <div class="text-xs text-fg-muted truncate" title={nft.collection ?? ''}>
            {nft.collection ?? '—'}
          </div>
          <div class="mt-2 text-xs text-fg-subtle flex items-center justify-between">
            <span>Floor</span>
            <span class="font-mono">{fmtUsd(nft.floor_usd)}</span>
          </div>
        </Card>
      {/each}
    </div>
  {/if}
</div>
