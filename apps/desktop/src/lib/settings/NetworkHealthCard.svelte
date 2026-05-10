<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { api, errorMessage, type NetworkHealth, type NetworkStatus } from '$lib/api';
  import { wallet } from '$lib/stores/wallet';
  import Card from '$lib/ui/Card.svelte';
  import Button from '$lib/ui/Button.svelte';

  let reports: NetworkHealth[] = [];
  let loading = false;
  let error = '';
  let lastRun: Date | null = null;

  /** Map chain id → human-readable name from the wallet store. */
  $: nameOf = (id: string) =>
    $wallet.chains.find((c) => c.id === id)?.display_name ?? id.toUpperCase();

  async function refresh() {
    loading = true;
    error = '';
    try {
      reports = await api.networkHealthAll();
      lastRun = new Date();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      loading = false;
    }
  }

  function dotClass(status: NetworkStatus): string {
    switch (status) {
      case 'synced':
        return 'bg-success';
      case 'lagging':
        return 'bg-warning';
      case 'offline':
        return 'bg-danger';
    }
  }

  function statusLabel(status: NetworkStatus): string {
    switch (status) {
      case 'synced':
        return 'Healthy';
      case 'lagging':
        return 'Lagging';
      case 'offline':
        return 'Offline';
    }
  }

  function fmt(ms: number | null): string {
    if (ms == null) return '—';
    return `${ms} ms`;
  }

  onMount(refresh);
  onDestroy(() => {
    /* nothing */
  });
</script>

<Card title="Network status" subtitle="Latency probe sends 5 head-height calls per chain.">
  <div class="flex items-center justify-between mb-4">
    <p class="text-xs text-fg-subtle">
      {#if lastRun}
        Last run {lastRun.toLocaleTimeString()}
      {:else}
        Not measured yet
      {/if}
    </p>
    <Button variant="secondary" size="sm" {loading} on:click={refresh}>
      {loading ? 'Probing…' : 'Refresh network'}
    </Button>
  </div>

  {#if error}
    <p class="text-sm text-danger mb-3">{error}</p>
  {/if}

  <div class="-mx-6 -mb-6 border-t border-border-subtle">
    {#each reports as r (r.chain_id)}
      <div class="px-6 py-3 border-b border-border-subtle last:border-b-0">
        <div class="flex items-center justify-between">
          <div class="flex items-center gap-3">
            <span class="h-2.5 w-2.5 rounded-full {dotClass(r.status)}"></span>
            <div>
              <div class="font-semibold text-sm flex items-center gap-2">
                {nameOf(r.chain_id)}
                {#if r.is_user_override}
                  <span
                    class="text-[9px] uppercase tracking-wider text-accent border border-accent/40 rounded px-1.5 py-px"
                    title="Using your custom RPC endpoint"
                  >
                    Custom
                  </span>
                {/if}
              </div>
              <div class="text-[11px] text-fg-subtle font-mono truncate max-w-[260px]">
                {r.endpoint || 'no endpoint configured'}
              </div>
            </div>
          </div>
          <div class="text-right text-xs">
            <div class="font-medium">{statusLabel(r.status)}</div>
            <div class="text-fg-subtle">
              {r.successes}/{r.samples} ok · p50 {fmt(r.latency_p50_ms)} · p95
              {fmt(r.latency_p95_ms)}
            </div>
            {#if r.head_height != null}
              <div class="text-fg-subtle font-mono">#{r.head_height}</div>
            {/if}
          </div>
        </div>
      </div>
    {/each}
    {#if reports.length === 0 && !loading}
      <div class="px-6 py-8 text-sm text-fg-muted text-center">No data yet.</div>
    {/if}
  </div>
</Card>
