<script lang="ts">
  import Card from '$lib/ui/Card.svelte';
  import Button from '$lib/ui/Button.svelte';
  import { api, errorMessage, type PaymentIntent } from '$lib/api';

  let raw = '';
  let result: PaymentIntent | null = null;
  let error = '';
  let busy = false;

  async function parse() {
    if (!raw.trim()) return;
    busy = true;
    error = '';
    result = null;
    try {
      result = await api.payuriParse(raw.trim());
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  function loadDemoBtc() {
    raw = 'bitcoin:bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq?amount=0.0125&label=Donation&message=Thanks';
    result = null;
    error = '';
  }

  function loadDemoEth() {
    raw = 'ethereum:0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48@1/transfer?address=0x6e1cE03Aef9C92aD3431106f2cC2db4cF9Aa9b66&uint256=1500000';
    result = null;
    error = '';
  }
</script>

<Card
  title="Payment URI parser"
  subtitle="Decodes BIP-21 (bitcoin:) and EIP-681 (ethereum:) payment URIs the way QR-code scanners produce them. Pure offline parsing — no network."
>
  <div class="space-y-3 text-sm">
    {#if error}
      <p class="text-rose-400 text-xs">{error}</p>
    {/if}

    <div class="flex items-center gap-2 flex-wrap">
      <Button variant="secondary" on:click={loadDemoBtc} disabled={busy}>Demo BTC</Button>
      <Button variant="secondary" on:click={loadDemoEth} disabled={busy}>Demo ETH</Button>
    </div>

    <textarea
      bind:value={raw}
      rows="3"
      placeholder={'bitcoin:bc1q...?amount=0.01   or   ethereum:0x...@1/transfer?address=0x...&uint256=1000000'}
      class="w-full px-3 py-2 rounded-md bg-bg-elevated border border-border-subtle text-fg font-mono text-[11px]"
    ></textarea>

    <Button variant="primary" on:click={parse} disabled={busy || !raw.trim()}>
      {busy ? 'Parsing…' : 'Parse URI'}
    </Button>

    {#if result}
      <div class="rounded-lg border border-border-subtle p-3 space-y-1.5 text-xs">
        {#if result.kind === 'Bitcoin'}
          <p class="font-medium text-amber-400">Bitcoin (BIP-21)</p>
          <p>
            <span class="text-fg-muted">Address:</span>
            <span class="font-mono break-all">{result.address}</span>
          </p>
          {#if result.amount_btc}
            <p>
              <span class="text-fg-muted">Amount:</span>
              <span class="font-mono">{result.amount_btc} BTC</span>
            </p>
          {/if}
          {#if result.label}
            <p><span class="text-fg-muted">Label:</span> {result.label}</p>
          {/if}
          {#if result.message}
            <p><span class="text-fg-muted">Message:</span> {result.message}</p>
          {/if}
        {:else if result.kind === 'Ethereum'}
          <p class="font-medium text-sky-400">Ethereum (EIP-681)</p>
          <p>
            <span class="text-fg-muted">Address:</span>
            <span class="font-mono break-all">{result.address}</span>
          </p>
          {#if result.chain_id !== null && result.chain_id !== undefined}
            <p>
              <span class="text-fg-muted">Chain ID:</span>
              <span class="font-mono">{result.chain_id}</span>
            </p>
          {/if}
          {#if result.function}
            <p>
              <span class="text-fg-muted">Function:</span>
              <span class="font-mono">{result.function}</span>
            </p>
          {/if}
          {#if result.token_recipient}
            <p>
              <span class="text-fg-muted">Token recipient:</span>
              <span class="font-mono break-all">{result.token_recipient}</span>
            </p>
          {/if}
          {#if result.amount_wei}
            <p>
              <span class="text-fg-muted">Amount (wei / token base units):</span>
              <span class="font-mono">{result.amount_wei}</span>
            </p>
          {/if}
          {#if result.gas}
            <p><span class="text-fg-muted">Gas:</span> <span class="font-mono">{result.gas}</span></p>
          {/if}
        {/if}
      </div>
    {/if}
  </div>
</Card>
