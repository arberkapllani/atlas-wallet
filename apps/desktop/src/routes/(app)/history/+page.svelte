<script lang="ts">
  import { onMount } from 'svelte';
  import Card from '$lib/ui/Card.svelte';
  import Button from '$lib/ui/Button.svelte';
  import { wallet } from '$lib/stores/wallet';
  import { commands, type TxRecord } from '$lib/bindings';

  let rows: TxRecord[] = [];
  let loading = false;
  let error = '';
  let chainFilter = '';
  let limit = 100;

  async function load() {
    loading = true;
    error = '';
    try {
      const res = await commands.txHistoryList(chainFilter, limit);
      if (res.status === 'ok') {
        rows = res.data;
      } else {
        error = 'message' in res.error ? res.error.message : res.error.kind;
      }
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  function exportCsv() {
    const header = [
      'time',
      'chain',
      'direction',
      'counterparty',
      'amount',
      'asset',
      'fee',
      'status',
      'txid',
      'memo'
    ].join(',');
    const lines = rows.map((r) =>
      [
        new Date(r.timestamp * 1000).toISOString(),
        r.chain_id,
        r.direction,
        r.counterparty,
        r.amount,
        r.asset,
        r.fee,
        r.status,
        r.txid,
        (r.memo ?? '').replace(/[",\n]/g, ' ')
      ]
        .map((v) => `"${String(v).replace(/"/g, '""')}"`)
        .join(',')
    );
    const blob = new Blob([[header, ...lines].join('\n')], { type: 'text/csv;charset=utf-8' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `greenwallet-history-${Date.now()}.csv`;
    a.click();
    URL.revokeObjectURL(url);
  }

  function fmtTime(ts: number): string {
    if (!ts) return '—';
    return new Date(ts * 1000).toLocaleString();
  }

  function statusClass(s: string): string {
    if (s === 'confirmed') return 'text-success';
    if (s === 'failed') return 'text-danger';
    return 'text-fg-muted';
  }

  function shorten(s: string, head = 8, tail = 6): string {
    if (!s) return '';
    if (s.length <= head + tail + 1) return s;
    return `${s.slice(0, head)}…${s.slice(-tail)}`;
  }

  onMount(load);
</script>

<div class="px-8 py-6 space-y-6">
  <header class="flex items-center justify-between">
    <div>
      <h2 class="text-2xl font-bold tracking-tight">Transaction history</h2>
      <p class="text-sm text-fg-muted">Sends recorded by GreenWallet, across every chain.</p>
    </div>
    <div class="flex items-center gap-2">
      <select
        bind:value={chainFilter}
        on:change={load}
        class="h-9 px-3 rounded-lg text-sm bg-bg-elevated border border-border-subtle"
      >
        <option value="">All chains</option>
        {#each $wallet.chains as c}
          <option value={c.id}>{c.display_name ?? c.id}</option>
        {/each}
      </select>
      <Button variant="secondary" on:click={exportCsv} disabled={!rows.length}>Export CSV</Button>
      <Button on:click={load} {loading}>Refresh</Button>
    </div>
  </header>

  {#if error}
    <div class="rounded-lg border border-danger/40 bg-danger/5 px-4 py-3 text-sm text-danger">
      {error}
    </div>
  {/if}

  <Card>
    {#if loading && !rows.length}
      <p class="text-sm text-fg-muted">Loading…</p>
    {:else if !rows.length}
      <p class="text-sm text-fg-muted">
        No transactions yet. Sends made through GreenWallet appear here.
      </p>
    {:else}
      <div class="overflow-x-auto">
        <table class="w-full text-sm">
          <thead class="text-xs uppercase tracking-wider text-fg-subtle">
            <tr>
              <th class="text-left py-2 pr-3">When</th>
              <th class="text-left py-2 pr-3">Chain</th>
              <th class="text-left py-2 pr-3">Direction</th>
              <th class="text-left py-2 pr-3">Counterparty</th>
              <th class="text-right py-2 pr-3">Amount</th>
              <th class="text-right py-2 pr-3">Fee</th>
              <th class="text-left py-2 pr-3">Status</th>
              <th class="text-left py-2">Tx</th>
            </tr>
          </thead>
          <tbody class="divide-y divide-border-subtle">
            {#each rows as row}
              <tr class="align-top">
                <td class="py-2 pr-3 whitespace-nowrap text-fg-muted">{fmtTime(row.timestamp)}</td>
                <td class="py-2 pr-3 font-mono text-xs">{row.chain_id}</td>
                <td class="py-2 pr-3 capitalize">{row.direction}</td>
                <td class="py-2 pr-3 font-mono text-xs">{shorten(row.counterparty)}</td>
                <td class="py-2 pr-3 text-right font-mono">{row.amount} {row.asset}</td>
                <td class="py-2 pr-3 text-right font-mono text-fg-muted">{row.fee}</td>
                <td class="py-2 pr-3 {statusClass(row.status)}">{row.status}</td>
                <td class="py-2 font-mono text-xs">{shorten(row.txid, 10, 8)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </Card>
</div>
