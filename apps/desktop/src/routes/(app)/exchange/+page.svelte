<script lang="ts">
  import { onDestroy } from 'svelte';
  import Card from '$lib/ui/Card.svelte';
  import Input from '$lib/ui/Input.svelte';
  import Button from '$lib/ui/Button.svelte';
  import {
    api,
    errorMessage,
    type ExchangeQuote,
    type ExchangeSwapResult,
    type SlippageBand
  } from '$lib/api';

  /**
   * Phase 5.1 — live swaps via 1inch v6 on Ethereum mainnet.
   * For ERC-20 sources Atlas auto-broadcasts the approve transaction
   * before the swap. Multi-chain expansion (Polygon/Arbitrum/Base/…) and
   * Thorchain cross-chain ship in subsequent phases.
   */

  type Token = {
    symbol: string;
    name: string;
    address: string;
    decimals: number;
  };

  // 1inch convention: native ETH uses this sentinel address.
  const NATIVE = '0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee';

  const TOKENS: Token[] = [
    { symbol: 'ETH', name: 'Ether', address: NATIVE, decimals: 18 },
    {
      symbol: 'USDC',
      name: 'USD Coin',
      address: '0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48',
      decimals: 6
    },
    {
      symbol: 'USDT',
      name: 'Tether',
      address: '0xdac17f958d2ee523a2206206994597c13d831ec7',
      decimals: 6
    },
    {
      symbol: 'WBTC',
      name: 'Wrapped BTC',
      address: '0x2260fac5e5542a773aa44fbcfedf7c193bc2c599',
      decimals: 8
    }
  ];

  const CHAIN_ID = 1; // Ethereum mainnet — Phase 5.0 scope.

  let fromSymbol = 'ETH';
  let toSymbol = 'USDC';
  let amount = '';
  let quote: ExchangeQuote | null = null;
  let loading = false;
  let error: string | null = null;
  let debounceHandle: ReturnType<typeof setTimeout> | null = null;

  let slippageBps = 100; // 1 % default
  let slippageBand: SlippageBand | null = null;
  let confirmOpen = false;
  let executing = false;
  let result: ExchangeSwapResult | null = null;

  $: fromTok = TOKENS.find((t) => t.symbol === fromSymbol)!;
  $: toTok = TOKENS.find((t) => t.symbol === toSymbol)!;

  function flip() {
    const next = fromSymbol;
    fromSymbol = toSymbol;
    toSymbol = next;
    quote = null;
    scheduleQuote();
  }

  /** Convert a decimal user input into base units (string) for the given token. */
  function toBaseUnits(input: string, decimals: number): string | null {
    const trimmed = input.trim();
    if (!trimmed) return null;
    if (!/^\d*(?:\.\d*)?$/.test(trimmed)) return null;
    const [whole, frac = ''] = trimmed.split('.');
    if (frac.length > decimals) return null;
    const padded = (frac + '0'.repeat(decimals)).slice(0, decimals);
    const joined = (whole || '0') + padded;
    const stripped = joined.replace(/^0+/, '') || '0';
    if (stripped === '0') return null;
    return stripped;
  }

  /** Format base units back to a decimal string (no thousand separators). */
  function fromBaseUnits(value: string, decimals: number): string {
    if (!value) return '0';
    const padded = value.padStart(decimals + 1, '0');
    const whole = padded.slice(0, padded.length - decimals).replace(/^0+/, '') || '0';
    const frac = padded.slice(padded.length - decimals).replace(/0+$/, '');
    return frac ? `${whole}.${frac}` : whole;
  }

  function scheduleQuote() {
    if (debounceHandle) clearTimeout(debounceHandle);
    debounceHandle = setTimeout(fetchQuote, 350);
  }

  async function fetchQuote() {
    error = null;
    quote = null;
    if (fromSymbol === toSymbol) {
      error = 'Pick two different tokens.';
      return;
    }
    const base = toBaseUnits(amount, fromTok.decimals);
    if (!base) return;
    loading = true;
    try {
      quote = await api.exchangeQuote({
        chain_id: CHAIN_ID,
        src: fromTok.address,
        dst: toTok.address,
        amount: base
      });
    } catch (e) {
      error = errorMessage(e);
    } finally {
      loading = false;
    }
  }

  onDestroy(() => {
    if (debounceHandle) clearTimeout(debounceHandle);
  });

  $: rate = (() => {
    if (!quote) return null;
    const inAmt = parseFloat(fromBaseUnits(quote.from_amount, fromTok.decimals));
    const outAmt = parseFloat(fromBaseUnits(quote.to_amount, toTok.decimals));
    if (!inAmt || !outAmt) return null;
    return outAmt / inAmt;
  })();

  function openConfirm() {
    if (!quote) return;
    result = null;
    error = null;
    confirmOpen = true;
  }

  async function executeSwap() {
    if (!quote) return;
    const base = toBaseUnits(amount, fromTok.decimals);
    if (!base) return;
    executing = true;
    error = null;
    try {
      result = await api.exchangeSwap({
        chain_id: CHAIN_ID,
        src: fromTok.address,
        dst: toTok.address,
        amount: base,
        slippage_bps: slippageBps,
        fee_level: 'normal'
      });
      // Keep modal open to show the txid; user closes manually.
    } catch (e) {
      error = errorMessage(e);
    } finally {
      executing = false;
    }
  }

  function closeConfirm() {
    if (executing) return;
    confirmOpen = false;
    if (result) {
      // Successful swap — clear amount so user starts fresh.
      amount = '';
      quote = null;
      result = null;
    }
  }

  /** Minimum out amount given current slippage tolerance, formatted. */
  $: minReceived = (() => {
    if (!quote) return null;
    const out = BigInt(quote.to_amount);
    const slipped = (out * BigInt(10_000 - slippageBps)) / 10_000n;
    return fromBaseUnits(slipped.toString(), toTok.decimals);
  })();

  // Surface the backend's classification of the chosen slippage. The
  // band drives both the colour and the label shown next to the
  // tolerance picker, so the user gets a one-glance read on how
  // aggressive the setting is.
  async function refreshBand(bps: number) {
    try {
      slippageBand = await api.slippageBand(bps);
    } catch {
      slippageBand = null;
    }
  }
  $: void refreshBand(slippageBps);

  function bandTone(b: SlippageBand | null): string {
    switch (b) {
      case 'Low':
        return 'text-success';
      case 'Normal':
        return 'text-fg';
      case 'High':
        return 'text-amber-400';
      case 'Reckless':
        return 'text-rose-400';
      default:
        return 'text-fg-muted';
    }
  }
</script>

<div class="p-8 max-w-2xl space-y-6">
  <header>
    <h1 class="text-2xl font-bold">Exchange</h1>
    <p class="text-fg-muted text-sm mt-1">
      Live aggregator quotes via 1inch v6. Ethereum mainnet — USDT, USDC, ETH, WBTC.
    </p>
  </header>

  <Card>
    <div class="space-y-4">
      <!-- From -->
      <div>
        <div class="flex items-center justify-between mb-2">
          <span class="text-sm text-fg-muted">You pay</span>
          <select
            bind:value={fromSymbol}
            on:change={scheduleQuote}
            class="bg-bg-elevated border border-border rounded-lg text-sm px-2 py-1
                   focus:outline-none focus:border-accent"
          >
            {#each TOKENS as t (t.symbol)}
              <option value={t.symbol}>{t.symbol}</option>
            {/each}
          </select>
        </div>
        <Input type="text" bind:value={amount} on:input={scheduleQuote} placeholder="0.0" />
      </div>

      <div class="flex justify-center">
        <button
          type="button"
          on:click={flip}
          class="h-9 w-9 rounded-full bg-bg-elevated border border-border
                 flex items-center justify-center text-fg-muted hover:text-fg
                 hover:border-accent transition"
          aria-label="Flip"
        >
          ⇅
        </button>
      </div>

      <!-- To -->
      <div>
        <div class="flex items-center justify-between mb-2">
          <span class="text-sm text-fg-muted">You receive (estimate)</span>
          <select
            bind:value={toSymbol}
            on:change={scheduleQuote}
            class="bg-bg-elevated border border-border rounded-lg text-sm px-2 py-1
                   focus:outline-none focus:border-accent"
          >
            {#each TOKENS as t (t.symbol)}
              <option value={t.symbol}>{t.symbol}</option>
            {/each}
          </select>
        </div>
        <div
          class="w-full bg-bg-elevated border border-border rounded-xl px-3.5 py-2.5
                 font-mono text-sm min-h-[42px] flex items-center"
        >
          {#if loading}
            <span class="text-fg-muted">Quoting…</span>
          {:else if quote}
            {fromBaseUnits(quote.to_amount, toTok.decimals)} {toSymbol}
          {:else}
            <span class="text-fg-subtle">—</span>
          {/if}
        </div>
      </div>

      {#if error}
        <p class="text-danger text-xs">{error}</p>
      {/if}

      {#if quote && rate !== null}
        <div class="border-t border-border-subtle pt-3 space-y-1.5 text-xs text-fg-muted">
          <div class="flex justify-between">
            <span>Rate</span>
            <span class="font-mono text-fg">
              1 {fromSymbol} ≈ {rate.toFixed(6)}
              {toSymbol}
            </span>
          </div>
          <div class="flex justify-between">
            <span>Estimated gas</span>
            <span class="font-mono text-fg">{quote.estimated_gas.toLocaleString()} units</span>
          </div>
          {#if quote.protocols.length}
            <div class="flex justify-between">
              <span>Routed through</span>
              <span class="text-fg text-right max-w-xs truncate" title={quote.protocols.join(', ')}>
                {quote.protocols.slice(0, 3).join(', ')}{quote.protocols.length > 3 ? '…' : ''}
              </span>
            </div>
          {/if}
        </div>
      {/if}

      <div class="flex items-center justify-between text-xs">
        <span class="text-fg-muted">Max slippage</span>
        <div class="flex items-center gap-2">
          {#if slippageBand}
            <span class="text-[10px] uppercase tracking-wider {bandTone(slippageBand)}">
              {slippageBand}
            </span>
          {/if}
          <div class="flex gap-1">
            {#each [50, 100, 300] as bps}
              <button
                type="button"
                on:click={() => (slippageBps = bps)}
                class="px-2.5 py-1 rounded-lg border text-xs transition
                     {slippageBps === bps
                  ? 'bg-accent/15 border-accent text-fg'
                  : 'bg-bg-elevated border-border text-fg-muted hover:border-accent'}"
              >
                {(bps / 100).toFixed(bps % 100 === 0 ? 0 : 1)}%
              </button>
            {/each}
          </div>
        </div>
      </div>

      <Button disabled={!quote || loading} on:click={openConfirm} fullWidth>
        {loading ? 'Quoting…' : 'Review swap'}
      </Button>
      <p class="text-[11px] text-fg-subtle text-center">
        GreenWallet signs and broadcasts on-device. ERC-20 sources auto-approve before the swap.
      </p>
    </div>
  </Card>

  <p class="text-xs text-fg-subtle">
    Tip: configure your 1inch API key in <a href="/settings" class="text-accent hover:underline"
      >Settings</a
    >
    for higher rate limits.
  </p>
</div>

{#if confirmOpen}
  <div
    class="fixed inset-0 z-50 bg-black/70 backdrop-blur-sm flex items-center justify-center p-4"
    on:click={closeConfirm}
    on:keydown={(e) => e.key === 'Escape' && closeConfirm()}
    role="presentation"
  >
    <div
      class="bg-bg-subtle border border-border-subtle rounded-2xl shadow-card w-full max-w-md p-6 space-y-4"
      on:click|stopPropagation
      on:keydown|stopPropagation
      role="dialog"
      aria-modal="true"
      tabindex="-1"
    >
      {#if !result}
        <h2 class="text-lg font-semibold">Confirm swap</h2>
        <div class="space-y-2 text-sm">
          <div class="flex justify-between">
            <span class="text-fg-muted">You pay</span>
            <span class="font-mono">{amount} {fromSymbol}</span>
          </div>
          <div class="flex justify-between">
            <span class="text-fg-muted">You receive (est.)</span>
            <span class="font-mono">
              {quote ? fromBaseUnits(quote.to_amount, toTok.decimals) : ''}
              {toSymbol}
            </span>
          </div>
          <div class="flex justify-between">
            <span class="text-fg-muted">Min received</span>
            <span class="font-mono text-fg">{minReceived} {toSymbol}</span>
          </div>
          <div class="flex justify-between">
            <span class="text-fg-muted">Slippage</span>
            <span>{(slippageBps / 100).toFixed(slippageBps % 100 === 0 ? 0 : 1)}%</span>
          </div>
          {#if fromSymbol !== 'ETH'}
            <p
              class="text-xs text-warning bg-warning/10 border border-warning/30 rounded-lg p-2.5 mt-3"
            >
              GreenWallet will broadcast a one-time
              <span class="font-mono">approve</span> for {amount}
              {fromSymbol} to the 1inch router before the swap.
            </p>
          {/if}
        </div>

        {#if error}
          <p class="text-danger text-xs break-all">{error}</p>
        {/if}

        <div class="flex gap-2 pt-2">
          <Button variant="secondary" on:click={closeConfirm} disabled={executing} fullWidth>
            Cancel
          </Button>
          <Button on:click={executeSwap} disabled={executing} fullWidth>
            {executing ? 'Broadcasting…' : 'Confirm swap'}
          </Button>
        </div>
      {:else}
        <h2 class="text-lg font-semibold">Swap broadcasted</h2>
        <div class="space-y-3 text-sm">
          {#if result.approve_txid}
            <div>
              <div class="text-fg-muted text-xs mb-1">Approve tx</div>
              <div
                class="font-mono text-xs break-all bg-bg-elevated border border-border rounded-lg p-2"
              >
                {result.approve_txid}
              </div>
            </div>
          {/if}
          <div>
            <div class="text-fg-muted text-xs mb-1">Swap tx</div>
            <div
              class="font-mono text-xs break-all bg-bg-elevated border border-border rounded-lg p-2"
            >
              {result.txid}
            </div>
          </div>
          <p class="text-xs text-fg-muted">
            Track confirmation in your block explorer of choice. Funds arrive once the swap
            transaction is mined.
          </p>
        </div>
        <Button on:click={closeConfirm} fullWidth>Done</Button>
      {/if}
    </div>
  </div>
{/if}
