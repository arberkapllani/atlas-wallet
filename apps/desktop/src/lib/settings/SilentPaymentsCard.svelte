<script lang="ts">
  import Card from '$lib/ui/Card.svelte';
  import Button from '$lib/ui/Button.svelte';
  import {
    api,
    errorMessage,
    type SpNetwork,
    type SilentPaymentAddress,
    type DemoSilentPayment
  } from '$lib/api';

  let network: SpNetwork = 'mainnet';
  let demo: DemoSilentPayment | null = null;
  let revealSecrets = false;
  let copiedAddress = false;
  let busy = false;
  let error = '';

  // Decode panel
  let decodeInput = '';
  let decoded: SilentPaymentAddress | null = null;
  let decodeError = '';

  // Re-derive panel
  let scanSecret = '';
  let spendSecret = '';
  let derivedAddress = '';
  let deriveError = '';

  async function generate() {
    busy = true;
    error = '';
    demo = null;
    revealSecrets = false;
    try {
      demo = await api.silentPaymentsGenerate(network);
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  async function copyAddress() {
    if (!demo) return;
    try {
      await navigator.clipboard.writeText(demo.address);
      copiedAddress = true;
      setTimeout(() => (copiedAddress = false), 1500);
    } catch (e) {
      error = errorMessage(e);
    }
  }

  async function decode() {
    decodeError = '';
    decoded = null;
    if (!decodeInput.trim()) return;
    busy = true;
    try {
      decoded = await api.silentPaymentsDecode(decodeInput.trim());
    } catch (e) {
      decodeError = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  async function derive() {
    deriveError = '';
    derivedAddress = '';
    busy = true;
    try {
      derivedAddress = await api.silentPaymentsAddressFromSecrets(
        network,
        scanSecret.trim(),
        spendSecret.trim()
      );
    } catch (e) {
      deriveError = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  function shortHex(s: string, head = 8, tail = 6): string {
    if (s.length <= head + tail + 1) return s;
    return `${s.slice(0, head)}…${s.slice(-tail)}`;
  }
</script>

<Card
  title="Silent payments (BIP352)"
  subtitle="Receive privately. The sender computes a fresh on-chain output for you each time so the same address is never reused on-chain."
>
  {#if error}
    <p class="text-sm text-rose-400">{error}</p>
  {/if}

  <section class="space-y-2">
    <div class="flex items-center gap-2 text-sm">
      <span class="font-semibold">Network</span>
      <select
        class="rounded bg-bg-elevated border border-border-subtle px-2 py-1 text-xs"
        aria-label="Silent-payment network"
        bind:value={network}
      >
        <option value="mainnet">Mainnet (sp1…)</option>
        <option value="testnet">Testnet (tsp1…)</option>
        <option value="regtest">Regtest (sprt1…)</option>
      </select>
      <Button on:click={generate} disabled={busy}>Generate receive address</Button>
    </div>

    {#if demo}
      <div class="space-y-2 rounded bg-bg-elevated p-3 font-mono text-xs">
        <div class="flex items-center gap-2">
          <span class="text-fg-subtle">Address</span>
          <code class="flex-1 break-all">{demo.address}</code>
          <Button on:click={copyAddress} variant="secondary">
            {copiedAddress ? '✓ Copied' : 'Copy'}
          </Button>
        </div>
        <div class="flex items-center gap-2">
          <span class="text-fg-subtle">Scan pub</span>
          <code class="flex-1 break-all">{demo.scan_pub_hex}</code>
        </div>
        <div class="flex items-center gap-2">
          <span class="text-fg-subtle">Spend pub</span>
          <code class="flex-1 break-all">{demo.spend_pub_hex}</code>
        </div>

        <div class="flex items-center gap-2 pt-2 border-t border-border-subtle">
          <label class="flex items-center gap-2">
            <input type="checkbox" bind:checked={revealSecrets} />
            <span>Reveal secret keys (back them up offline)</span>
          </label>
        </div>
        {#if revealSecrets}
          <div class="flex items-center gap-2">
            <span class="text-rose-400">scan secret</span>
            <code class="flex-1 break-all">{demo.scan_secret_hex}</code>
          </div>
          <div class="flex items-center gap-2">
            <span class="text-rose-400">spend secret</span>
            <code class="flex-1 break-all">{demo.spend_secret_hex}</code>
          </div>
        {:else}
          <div class="flex items-center gap-2 text-fg-subtle">
            <span>scan/spend secrets</span>
            <code class="flex-1"
              >{shortHex(demo.scan_secret_hex)} · {shortHex(demo.spend_secret_hex)}</code
            >
          </div>
        {/if}
      </div>
    {/if}
  </section>

  <section class="space-y-2 border-t border-border-subtle pt-4">
    <h3 class="text-sm font-semibold">Re-derive an address from secrets</h3>
    <div class="grid grid-cols-1 gap-2 sm:grid-cols-2">
      <input
        class="rounded bg-bg-elevated border border-border-subtle px-2 py-1 text-xs font-mono"
        placeholder="scan secret hex (32 bytes)"
        bind:value={scanSecret}
      />
      <input
        class="rounded bg-bg-elevated border border-border-subtle px-2 py-1 text-xs font-mono"
        placeholder="spend secret hex (32 bytes)"
        bind:value={spendSecret}
      />
    </div>
    <Button on:click={derive} disabled={busy || !scanSecret || !spendSecret}>Derive</Button>
    {#if deriveError}
      <p class="text-xs text-rose-400">{deriveError}</p>
    {:else if derivedAddress}
      <code class="block break-all rounded bg-bg-elevated p-2 text-xs">
        {derivedAddress}
      </code>
    {/if}
  </section>

  <section class="space-y-2 border-t border-border-subtle pt-4">
    <h3 class="text-sm font-semibold">Decode an address</h3>
    <div class="flex gap-2">
      <input
        class="flex-1 rounded bg-bg-elevated border border-border-subtle px-2 py-1 text-xs font-mono"
        placeholder="sp1… / tsp1… / sprt1…"
        bind:value={decodeInput}
      />
      <Button on:click={decode} disabled={busy || !decodeInput.trim()}>Decode</Button>
    </div>
    {#if decodeError}
      <p class="text-xs text-rose-400">{decodeError}</p>
    {:else if decoded}
      <div class="space-y-1 rounded bg-bg-elevated p-2 font-mono text-xs">
        <div>network: <span class="text-emerald-400">{decoded.network}</span></div>
        <div class="break-all">scan_pub: {decoded.scan_pub_hex}</div>
        <div class="break-all">spend_pub: {decoded.spend_pub_hex}</div>
      </div>
    {/if}
  </section>
</Card>
