<script lang="ts">
  import { onMount } from 'svelte';
  import { wallet } from '$lib/stores/wallet';
  import {
    api,
    parseAmountToBase,
    formatAmount,
    errorMessage,
    type FeeOption,
    type TokenSummary,
    type ChainSummary
  } from '$lib/api';
  import Button from '$lib/ui/Button.svelte';
  import Card from '$lib/ui/Card.svelte';
  import Input from '$lib/ui/Input.svelte';

  /** A unified entry the user can choose to send: a chain's native asset
   * or one of its tokens. */
  interface SendableAsset {
    key: string; // unique form key, e.g. "native:eth" or "token:usdt-erc20"
    kind: 'native' | 'token';
    chainId: string;
    chainName: string;
    symbol: string;
    label: string;
    decimals: number;
    /** Set only for token entries. */
    tokenId?: string;
  }

  let tokens: TokenSummary[] = [];
  let selectedKey = '';
  let to = '';
  let amount = '';
  let feeOptions: FeeOption[] = [];
  let selectedLevel = 'normal';
  let busy = false;
  let error = '';
  let result: { txid: string } | null = null;

  onMount(async () => {
    try {
      tokens = await api.listTokens();
    } catch (e) {
      console.warn('token list failed', e);
    }
  });

  $: assets = buildAssets($wallet.chains, tokens);
  $: selected = assets.find((a) => a.key === selectedKey);

  function buildAssets(
    chains: ChainSummary[],
    allTokens: TokenSummary[]
  ): SendableAsset[] {
    const out: SendableAsset[] = [];
    for (const c of chains) {
      out.push({
        key: `native:${c.id}`,
        kind: 'native',
        chainId: c.id,
        chainName: c.display_name,
        symbol: c.symbol,
        label: `${c.symbol} — ${c.display_name}`,
        decimals: c.decimals
      });
      for (const t of allTokens) {
        if (t.chain_id !== c.id) continue;
        if (!t.enabled_by_default) continue;
        out.push({
          key: `token:${t.id}`,
          kind: 'token',
          chainId: c.id,
          chainName: c.display_name,
          symbol: t.symbol,
          label: `${t.symbol} — ${t.display_name}`,
          decimals: t.decimals,
          tokenId: t.id
        });
      }
    }
    return out;
  }

  async function loadFees(chainId: string) {
    feeOptions = [];
    try {
      feeOptions = await api.getFeeOptions(chainId);
    } catch (e) {
      console.warn('fee fetch failed', e);
    }
  }

  $: if (selected) void loadFees(selected.chainId);

  async function send() {
    if (!selected) return;
    error = '';
    result = null;
    let baseAmount: string;
    try {
      baseAmount = parseAmountToBase(amount, selected.decimals);
    } catch (e) {
      error = errorMessage(e);
      return;
    }
    busy = true;
    try {
      let r;
      if (selected.kind === 'native') {
        r = await api.sendNative({
          chain_id: selected.chainId,
          to,
          amount: baseAmount,
          fee_level: selectedLevel
        });
      } else {
        r = await api.sendToken({
          token_id: selected.tokenId!,
          to,
          amount: baseAmount,
          fee_level: selectedLevel
        });
      }
      result = { txid: r.txid };
      void wallet.refreshBalances();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }
</script>

<div class="p-8 max-w-2xl space-y-6">
  <header>
    <h1 class="text-2xl font-bold">Send</h1>
    <p class="text-fg-muted text-sm mt-1">
      Transfer the native asset or a token (USDT, USDC, …) on any supported chain.
    </p>
  </header>

  <Card>
    <div class="space-y-4">
      <div>
        <span class="text-sm text-fg-muted block mb-2">Asset</span>
        <select
          bind:value={selectedKey}
          class="w-full bg-bg-elevated border border-border rounded-xl px-3.5 py-2.5 text-sm focus:outline-none focus:border-accent"
        >
          <option value="" disabled>Select an asset</option>
          {#each assets as a (a.key)}
            <option value={a.key}>{a.label}</option>
          {/each}
        </select>
        {#if selected}
          <p class="text-xs text-fg-subtle mt-1.5">
            {selected.kind === 'native' ? 'Native asset' : 'Token'} on {selected.chainName}
          </p>
        {/if}
      </div>

      <Input label="Recipient address" bind:value={to} placeholder="bc1q… / 0x… / T…" />

      <Input
        label="Amount"
        type="text"
        bind:value={amount}
        placeholder="0.0"
        hint={selected
          ? `In ${selected.symbol}. Up to ${selected.decimals} decimal places.`
          : undefined}
      />

      {#if feeOptions.length > 0}
        <div>
          <span class="text-sm text-fg-muted block mb-2">Network fee</span>
          <div class="grid grid-cols-3 gap-2">
            {#each feeOptions as opt}
              <button
                class="rounded-xl px-3 py-3 border text-left transition {selectedLevel === opt.level ? 'border-accent bg-accent/10' : 'border-border bg-bg-elevated'}"
                on:click={() => (selectedLevel = opt.level)}
              >
                <div class="text-sm font-semibold capitalize">{opt.level}</div>
                <div class="text-xs text-fg-muted font-mono mt-1">{formatAmount(opt.estimated_fee)}</div>
                <div class="text-[10px] text-fg-subtle mt-0.5">~{Math.max(1, Math.round(opt.eta_seconds / 60))} min</div>
              </button>
            {/each}
          </div>
          {#if selected?.kind === 'token'}
            <p class="text-xs text-fg-subtle mt-2">
              Tokens are paid for in the chain's native asset (gas / energy).
            </p>
          {/if}
        </div>
      {/if}

      {#if error}<p class="text-sm text-danger">{error}</p>{/if}
      {#if result}
        <div class="rounded-xl border border-success/30 bg-success/10 p-4">
          <div class="text-sm font-semibold text-success">Broadcast successful</div>
          <div class="text-xs text-fg-muted mt-1 font-mono break-all">{result.txid}</div>
        </div>
      {/if}

      <Button
        fullWidth
        loading={busy}
        disabled={!selected || !to || !amount}
        on:click={send}
      >
        Review &amp; send
      </Button>
    </div>
  </Card>
</div>
