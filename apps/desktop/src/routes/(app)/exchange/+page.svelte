<script lang="ts">
  import { onDestroy } from 'svelte';
  import Card from '$lib/ui/Card.svelte';
  import Input from '$lib/ui/Input.svelte';
  import Button from '$lib/ui/Button.svelte';
  import { api, errorMessage, type ExchangeQuote } from '$lib/api';

  /**
   * Phase 5.0 — quote-only via 1inch v6 (Ethereum mainnet).
   * Execution (sign + broadcast of the swap calldata, ERC-20 approvals)
   * lands in Phase 5.1 once the EVM signer exposes arbitrary contract
   * calls.
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
        <Input
          type="text"
          bind:value={amount}
          on:input={scheduleQuote}
          placeholder="0.0"
        />
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
              1 {fromSymbol} ≈ {rate.toFixed(6)} {toSymbol}
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

      <Button disabled fullWidth>
        Execute swap (Phase 5.1)
      </Button>
      <p class="text-[11px] text-fg-subtle text-center">
        Quote-only in 5.0. Sign + broadcast of the aggregator transaction lands in 5.1.
      </p>
    </div>
  </Card>

  <p class="text-xs text-fg-subtle">
    Tip: configure your 1inch API key in <a href="/settings" class="text-accent hover:underline">Settings</a>
    for higher rate limits.
  </p>
</div>
