<script lang="ts">
  import Card from '$lib/ui/Card.svelte';
  import Button from '$lib/ui/Button.svelte';
  import { api, errorMessage, type Share } from '$lib/api';

  let secretHex = '';
  let threshold = 3;
  let total = 5;
  let shares: Share[] = [];
  let combineInput = '';
  let recovered = '';
  let error = '';
  let busy = false;

  async function split() {
    if (!secretHex.trim()) return;
    busy = true;
    error = '';
    shares = [];
    try {
      shares = await api.shamirSplit(
        secretHex.trim().replace(/^0x/, ''),
        Number(threshold),
        Number(total)
      );
      combineInput = JSON.stringify(shares, null, 2);
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  async function combine() {
    if (!combineInput.trim()) return;
    busy = true;
    error = '';
    recovered = '';
    try {
      const parsed = JSON.parse(combineInput) as Share[];
      recovered = await api.shamirCombine(parsed);
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  function loadDemo() {
    secretHex = '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef';
    threshold = 3;
    total = 5;
  }
</script>

<Card
  title="Shamir secret sharing"
  subtitle="SLIP-39-style GF(256) secret splitter. Split a hex secret into N shares with a recovery threshold T; any T of N reconstruct it. Pure offline math; OS RNG seeds the polynomial coefficients."
>
  <div class="space-y-3 text-sm">
    {#if error}
      <p class="text-rose-400 text-xs">{error}</p>
    {/if}

    <div class="rounded-lg border border-border-subtle p-3 space-y-2">
      <p class="text-xs text-fg-muted font-medium">Split</p>
      <div class="flex items-center gap-2">
        <input
          type="text"
          bind:value={secretHex}
          placeholder="hex secret (no 0x prefix)"
          class="flex-1 h-9 px-2 rounded-md bg-bg-elevated border border-border-subtle text-fg text-xs font-mono"
        />
        <Button variant="secondary" on:click={loadDemo} disabled={busy}>Demo</Button>
      </div>
      <div class="grid grid-cols-2 gap-2">
        <label class="text-xs text-fg-muted block">
          Threshold (T)
          <input type="number" bind:value={threshold} min="1"
            class="mt-1 w-full h-9 px-2 rounded-md bg-bg-elevated border border-border-subtle text-fg text-xs font-mono" />
        </label>
        <label class="text-xs text-fg-muted block">
          Total shares (N)
          <input type="number" bind:value={total} min="1"
            class="mt-1 w-full h-9 px-2 rounded-md bg-bg-elevated border border-border-subtle text-fg text-xs font-mono" />
        </label>
      </div>
      <Button variant="primary" on:click={split} disabled={busy || !secretHex.trim()}>
        Split into {total} shares (T={threshold})
      </Button>
      {#if shares.length > 0}
        <ul class="border border-border-subtle rounded-md divide-y divide-border-subtle text-[11px]">
          {#each shares as s}
            <li class="px-2 py-1.5 flex gap-2">
              <span class="font-mono text-fg-muted">x={s.x}</span>
              <span class="font-mono break-all">{s.y_hex}</span>
            </li>
          {/each}
        </ul>
      {/if}
    </div>

    <div class="rounded-lg border border-border-subtle p-3 space-y-2">
      <p class="text-xs text-fg-muted font-medium">Combine</p>
      <textarea
        bind:value={combineInput}
        rows="4"
        placeholder={'[{"x":1,"y_hex":"..."},{"x":2,"y_hex":"..."}]'}
        class="w-full px-2 py-2 rounded-md bg-bg-elevated border border-border-subtle text-fg font-mono text-[11px]"
      ></textarea>
      <Button variant="primary" on:click={combine} disabled={busy || !combineInput.trim()}>
        Recover secret
      </Button>
      {#if recovered}
        <p class="text-xs">
          <span class="text-fg-muted">Recovered:</span>
          <span class="font-mono break-all">{recovered}</span>
        </p>
      {/if}
    </div>
  </div>
</Card>
