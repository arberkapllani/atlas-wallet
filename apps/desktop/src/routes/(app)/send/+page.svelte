<script lang="ts">
  import { wallet } from '$lib/stores/wallet';
  import { api, parseAmountToBase, formatAmount, errorMessage, type FeeOption } from '$lib/api';
  import Button from '$lib/ui/Button.svelte';
  import Card from '$lib/ui/Card.svelte';
  import Input from '$lib/ui/Input.svelte';

  let chainId = '';
  let to = '';
  let amount = '';
  let feeOptions: FeeOption[] = [];
  let selectedLevel = 'normal';
  let busy = false;
  let error = '';
  let result: { txid: string } | null = null;

  $: chain = $wallet.chains.find((c) => c.id === chainId);

  async function loadFees() {
    feeOptions = [];
    if (!chainId) return;
    try {
      feeOptions = await api.getFeeOptions(chainId);
    } catch (e) {
      console.warn('fee fetch failed', e);
    }
  }

  $: if (chainId) void loadFees();

  async function send() {
    if (!chain) return;
    error = '';
    result = null;
    let baseAmount: string;
    try {
      baseAmount = parseAmountToBase(amount, chain.decimals);
    } catch (e) {
      error = errorMessage(e);
      return;
    }
    busy = true;
    try {
      const r = await api.sendNative({
        chain_id: chainId,
        to,
        amount: baseAmount,
        fee_level: selectedLevel
      });
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
    <p class="text-fg-muted text-sm mt-1">Transfer the native asset of any supported chain.</p>
  </header>

  <Card>
    <div class="space-y-4">
      <div>
        <span class="text-sm text-fg-muted block mb-2">Chain</span>
        <select
          bind:value={chainId}
          class="w-full bg-bg-elevated border border-border rounded-xl px-3.5 py-2.5 text-sm focus:outline-none focus:border-accent"
        >
          <option value="" disabled>Select a chain</option>
          {#each $wallet.chains as c}
            <option value={c.id}>{c.display_name} ({c.symbol})</option>
          {/each}
        </select>
      </div>

      <Input label="Recipient address" bind:value={to} placeholder="bc1q… / 0x…" />

      <Input
        label="Amount"
        type="text"
        bind:value={amount}
        placeholder="0.0"
        hint={chain ? `In ${chain.symbol}. Up to ${chain.decimals} decimal places.` : undefined}
      />

      {#if feeOptions.length > 0}
        <div>
          <span class="text-sm text-fg-muted block mb-2">Fee</span>
          <div class="grid grid-cols-3 gap-2">
            {#each feeOptions as opt}
              <button
                class="rounded-xl px-3 py-3 border text-left transition {selectedLevel === opt.level ? 'border-accent bg-accent/10' : 'border-border bg-bg-elevated'}"
                on:click={() => (selectedLevel = opt.level)}
              >
                <div class="text-sm font-semibold capitalize">{opt.level}</div>
                <div class="text-xs text-fg-muted font-mono mt-1">{formatAmount(opt.estimated_fee)}</div>
                <div class="text-[10px] text-fg-subtle mt-0.5">~{Math.round(opt.eta_seconds / 60)} min</div>
              </button>
            {/each}
          </div>
        </div>
      {/if}

      {#if error}<p class="text-sm text-danger">{error}</p>{/if}
      {#if result}
        <div class="rounded-xl border border-success/30 bg-success/10 p-4">
          <div class="text-sm font-semibold text-success">Broadcast successful</div>
          <div class="text-xs text-fg-muted mt-1 font-mono break-all">{result.txid}</div>
        </div>
      {/if}

      <Button fullWidth loading={busy} disabled={!chainId || !to || !amount} on:click={send}>
        Review &amp; send
      </Button>
    </div>
  </Card>
</div>
