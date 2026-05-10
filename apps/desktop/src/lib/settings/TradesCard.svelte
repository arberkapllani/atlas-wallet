<script lang="ts">
  import { onMount } from 'svelte';
  import Card from '$lib/ui/Card.svelte';
  import Button from '$lib/ui/Button.svelte';
  import { prices } from '$lib/stores/prices';
  import {
    api,
    errorMessage,
    type Trade,
    type TradeKind,
    type AccountingMethod,
    type PortfolioReport
  } from '$lib/api';

  let trades: Trade[] = [];
  let report: PortfolioReport | null = null;
  let error = '';
  let busy = false;

  // Add-trade form.
  let formOpen = false;
  let newKind: TradeKind = 'Buy';
  let newAsset = 'btc';
  let newQty = '';
  let newPrice = '';
  let newTs = new Date().toISOString().slice(0, 16);

  // Import.
  let importOpen = false;
  let importText = '';

  // P&L config.
  let method: AccountingMethod = 'Fifo';

  async function load() {
    error = '';
    try {
      trades = await api.tradesList();
    } catch (e) {
      error = errorMessage(e);
    }
  }

  function priceMap(): Record<string, number> {
    const out: Record<string, number> = {};
    for (const [k, v] of Object.entries($prices)) {
      out[k] = v.price;
    }
    return out;
  }

  async function add() {
    const qty = Number(newQty);
    const px = Number(newPrice);
    const ts = Math.floor(new Date(newTs).getTime() / 1000);
    if (!Number.isFinite(qty) || qty <= 0) {
      error = 'Quantity must be a positive number.';
      return;
    }
    if (!Number.isFinite(px) || px < 0) {
      error = 'Unit price must be a non-negative number.';
      return;
    }
    if (!Number.isFinite(ts)) {
      error = 'Timestamp is invalid.';
      return;
    }
    busy = true;
    error = '';
    try {
      const trade: Trade = {
        kind: newKind,
        asset: newAsset.trim().toLowerCase() || 'btc',
        quantity: qty,
        unit_price_usd: px,
        ts
      };
      await api.tradesAdd(trade);
      newQty = '';
      newPrice = '';
      formOpen = false;
      await load();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  async function remove(index: number) {
    busy = true;
    error = '';
    try {
      await api.tradesRemove(index);
      await load();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  async function clearAll() {
    if (!confirm('Clear every recorded trade?')) return;
    busy = true;
    error = '';
    try {
      await api.tradesClear();
      report = null;
      await load();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  async function importJson() {
    if (!importText.trim()) return;
    busy = true;
    error = '';
    try {
      await api.tradesImportJson(importText);
      importText = '';
      importOpen = false;
      await load();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  async function compute() {
    busy = true;
    error = '';
    report = null;
    try {
      report = await api.tradesComputePnl(priceMap(), method);
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
      maximumFractionDigits: 2
    });
  }

  function fmtTs(ts: number): string {
    return new Date(ts * 1000).toLocaleString();
  }

  onMount(load);
</script>

<Card
  title="Trade history & P&L"
  subtitle="Manually record buys/sells (or paste a JSON snapshot) and compute realized + unrealized P&L using FIFO / LIFO / Average-cost. On-chain auto-import lands in a later phase."
>
  <div class="space-y-3 text-sm">
    {#if error}
      <p class="text-rose-400 text-xs">{error}</p>
    {/if}

    <div class="flex items-center gap-2 flex-wrap">
      <Button variant="primary" on:click={() => (formOpen = !formOpen)} disabled={busy}>
        {formOpen ? 'Cancel' : 'Add trade'}
      </Button>
      <Button variant="secondary" on:click={() => (importOpen = !importOpen)} disabled={busy}>
        {importOpen ? 'Cancel import' : 'Import JSON'}
      </Button>
      {#if trades.length > 0}
        <Button variant="secondary" on:click={clearAll} disabled={busy}>Clear all</Button>
      {/if}
      <span class="text-fg-subtle text-xs ml-auto">{trades.length} trades</span>
    </div>

    {#if formOpen}
      <div class="rounded-lg border border-border-subtle p-3 space-y-2">
        <div class="grid grid-cols-2 gap-2">
          <label class="text-xs text-fg-muted block">
            Kind
            <select
              bind:value={newKind}
              class="mt-1 w-full h-9 px-2 rounded-md bg-bg-elevated border border-border-subtle text-fg text-xs"
            >
              <option value="Buy">Buy</option>
              <option value="Sell">Sell</option>
            </select>
          </label>
          <label class="text-xs text-fg-muted block">
            Asset (chain id)
            <input
              type="text"
              bind:value={newAsset}
              placeholder="btc"
              class="mt-1 w-full h-9 px-2 rounded-md bg-bg-elevated border border-border-subtle text-fg text-xs font-mono"
            />
          </label>
          <label class="text-xs text-fg-muted block">
            Quantity
            <input
              type="number"
              step="any"
              bind:value={newQty}
              class="mt-1 w-full h-9 px-2 rounded-md bg-bg-elevated border border-border-subtle text-fg text-xs font-mono"
            />
          </label>
          <label class="text-xs text-fg-muted block">
            Unit price (USD)
            <input
              type="number"
              step="any"
              bind:value={newPrice}
              class="mt-1 w-full h-9 px-2 rounded-md bg-bg-elevated border border-border-subtle text-fg text-xs font-mono"
            />
          </label>
        </div>
        <label class="text-xs text-fg-muted block">
          Timestamp
          <input
            type="datetime-local"
            bind:value={newTs}
            class="mt-1 w-full h-9 px-2 rounded-md bg-bg-elevated border border-border-subtle text-fg text-xs font-mono"
          />
        </label>
        <Button variant="primary" on:click={add} disabled={busy}>Add to history</Button>
      </div>
    {/if}

    {#if importOpen}
      <div class="rounded-lg border border-border-subtle p-3 space-y-2">
        <p class="text-xs text-fg-muted">
          Paste a JSON array of <span class="font-mono">Trade</span> objects.
        </p>
        <textarea
          bind:value={importText}
          rows="4"
          class="w-full px-3 py-2 rounded-md bg-bg-elevated border border-border-subtle text-fg font-mono text-[11px]"
        ></textarea>
        <Button variant="primary" on:click={importJson} disabled={busy || !importText.trim()}>
          Import
        </Button>
      </div>
    {/if}

    {#if trades.length === 0}
      <p class="text-fg-subtle text-xs">No trades recorded.</p>
    {:else}
      <ul
        class="border border-border-subtle rounded-lg divide-y divide-border-subtle max-h-64 overflow-y-auto"
      >
        {#each trades as t, i}
          <li class="px-3 py-2 flex items-center gap-3 text-xs">
            <span class="font-medium {t.kind === 'Buy' ? 'text-emerald-400' : 'text-rose-400'}">
              {t.kind}
            </span>
            <span class="font-mono uppercase">{t.asset}</span>
            <span class="font-mono">{t.quantity}</span>
            <span class="font-mono text-fg-muted">@ {fmtUsd(t.unit_price_usd)}</span>
            <span class="text-[10px] text-fg-subtle ml-auto">{fmtTs(t.ts)}</span>
            <button
              type="button"
              on:click={() => remove(i)}
              disabled={busy}
              class="text-[10px] text-rose-400 hover:text-rose-300"
            >
              Remove
            </button>
          </li>
        {/each}
      </ul>
    {/if}

    <div class="flex items-center gap-2 pt-1">
      <label class="text-xs text-fg-muted">
        Method
        <select
          bind:value={method}
          class="ml-2 h-9 px-2 rounded-md bg-bg-elevated border border-border-subtle text-fg text-xs"
        >
          <option value="Fifo">FIFO</option>
          <option value="Lifo">LIFO</option>
          <option value="AverageCost">Average cost</option>
        </select>
      </label>
      <Button variant="primary" on:click={compute} disabled={busy || trades.length === 0}>
        Compute P&L
      </Button>
    </div>

    {#if report}
      <div class="rounded-lg border border-border-subtle p-3 space-y-2">
        <div class="grid grid-cols-2 gap-2 text-xs">
          <div>
            <p class="text-fg-muted">Realized</p>
            <p
              class="font-mono {report.total_realized_usd >= 0
                ? 'text-emerald-400'
                : 'text-rose-400'}"
            >
              {fmtUsd(report.total_realized_usd)}
            </p>
          </div>
          <div>
            <p class="text-fg-muted">Unrealized</p>
            <p
              class="font-mono {report.total_unrealized_usd >= 0
                ? 'text-emerald-400'
                : 'text-rose-400'}"
            >
              {fmtUsd(report.total_unrealized_usd)}
            </p>
          </div>
        </div>
        {#if report.positions.length > 0}
          <p class="text-[10px] text-fg-muted mt-2">Positions still held</p>
          <ul class="border border-border-subtle rounded-md divide-y divide-border-subtle">
            {#each report.positions as p}
              <li class="px-2 py-1.5 text-xs flex items-center gap-2">
                <span class="font-mono uppercase">{p.asset}</span>
                <span class="font-mono">{p.quantity_held}</span>
                <span class="text-fg-subtle text-[10px] ml-auto">
                  cost: <span class="font-mono">{fmtUsd(p.cost_basis_usd)}</span>
                  &middot; mkt: <span class="font-mono">{fmtUsd(p.market_value_usd)}</span>
                  &middot;
                  <span
                    class="font-mono {p.unrealized_pnl_usd >= 0
                      ? 'text-emerald-400'
                      : 'text-rose-400'}"
                  >
                    {fmtUsd(p.unrealized_pnl_usd)}
                  </span>
                </span>
              </li>
            {/each}
          </ul>
        {/if}
        {#if report.realized.length > 0}
          <p class="text-[10px] text-fg-muted mt-2">Realized events ({report.realized.length})</p>
        {/if}
      </div>
    {/if}
  </div>
</Card>
