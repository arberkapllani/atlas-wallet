<script lang="ts">
  import { onMount } from 'svelte';
  import Card from '$lib/ui/Card.svelte';
  import Button from '$lib/ui/Button.svelte';
  import {
    api,
    errorMessage,
    type Origin,
    type UtxoRef,
    type UtxoLabel,
    type LabeledUtxo,
    type LabeledUtxoEntry,
    type MixWarning,
    type SelectionStrategy,
    type Selection
  } from '$lib/api';

  // Persistent label store rendered as a list. Each row is a
  // (UtxoRef, UtxoLabel) pair editable in place.
  let entries: LabeledUtxoEntry[] = [];
  let error = '';
  let busy = false;

  // Free-form add row.
  let newTxid = '';
  let newVout = 0;
  let newOrigin: Origin = 'unknown';
  let newNote = '';
  let newTags = '';

  // Selection planner inputs. Users paste a JSON array of
  // LabeledUtxo (or load a demo set) and ask the wallet to either
  // detect mixing problems or suggest a single-bucket selection.
  let poolJson = '';
  let warnings: MixWarning[] = [];
  let selection: Selection | null = null;
  let target = 0;
  let strategy: SelectionStrategy = 'largest_first';

  const ORIGINS: Origin[] = ['unknown', 'kyc_tainted', 'p2p', 'mining', 'private', 'donation'];
  const STRATEGIES: SelectionStrategy[] = ['smallest_first', 'largest_first', 'branch_and_bound'];

  function originLabel(o: Origin): string {
    switch (o) {
      case 'kyc_tainted':
        return 'KYC-tainted';
      case 'p2p':
        return 'P2P';
      case 'private':
        return 'Private (CoinJoin/PayJoin)';
      case 'mining':
        return 'Mining';
      case 'donation':
        return 'Donation';
      default:
        return 'Unknown';
    }
  }

  async function refresh() {
    busy = true;
    error = '';
    try {
      entries = await api.coincontrolLabelList();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  async function addEntry() {
    if (!newTxid.trim()) {
      error = 'txid required';
      return;
    }
    busy = true;
    error = '';
    try {
      const utxo: UtxoRef = { txid: newTxid.trim(), vout: Math.max(0, Math.floor(newVout)) };
      const label: UtxoLabel = {
        origin: newOrigin,
        note: newNote,
        tags: newTags
          .split(',')
          .map((t) => t.trim())
          .filter((t) => t.length > 0)
      };
      await api.coincontrolLabelUpsert(utxo, label);
      newTxid = '';
      newVout = 0;
      newNote = '';
      newTags = '';
      newOrigin = 'unknown';
      await refresh();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  async function updateRow(entry: LabeledUtxoEntry) {
    busy = true;
    error = '';
    try {
      await api.coincontrolLabelUpsert(entry.utxo, entry.label);
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  async function removeRow(entry: LabeledUtxoEntry) {
    busy = true;
    error = '';
    try {
      await api.coincontrolLabelRemove(entry.utxo);
      await refresh();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  function loadDemoPool() {
    const demo: LabeledUtxo[] = [
      {
        utxo: { txid: 'aa'.repeat(32), vout: 0 },
        value: 50_000,
        confirmations: 6,
        label: { origin: 'kyc_tainted', note: 'CEX withdrawal', tags: ['exchange'] }
      },
      {
        utxo: { txid: 'bb'.repeat(32), vout: 1 },
        value: 80_000,
        confirmations: 12,
        label: { origin: 'private', note: 'CoinJoin output', tags: ['mixed'] }
      },
      {
        utxo: { txid: 'cc'.repeat(32), vout: 0 },
        value: 30_000,
        confirmations: 3,
        label: { origin: 'p2p', note: 'Bisq trade', tags: [] }
      },
      {
        utxo: { txid: 'dd'.repeat(32), vout: 2 },
        value: 15_000,
        confirmations: 1,
        label: { origin: 'unknown', note: '', tags: [] }
      }
    ];
    poolJson = JSON.stringify(demo, null, 2);
  }

  function parsePool(): LabeledUtxo[] {
    try {
      const parsed = JSON.parse(poolJson);
      if (!Array.isArray(parsed)) throw new Error('expected JSON array');
      return parsed as LabeledUtxo[];
    } catch (e) {
      throw new Error(`invalid pool JSON: ${(e as Error).message}`);
    }
  }

  async function detectMix() {
    busy = true;
    error = '';
    warnings = [];
    try {
      const pool = parsePool();
      warnings = await api.coincontrolDetectMix(pool);
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  async function suggestSelection() {
    busy = true;
    error = '';
    selection = null;
    try {
      const pool = parsePool();
      selection = await api.coincontrolSuggestSelection(target, pool, strategy);
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  function warningLabel(w: MixWarning): { text: string; tone: string } {
    if (w.kind === 'kyc_meets_private') {
      return {
        text: `KYC-tainted UTXO mixed with private output — destroys privacy.`,
        tone: 'rose'
      };
    }
    if (w.kind === 'kyc_meets_p2p') {
      return {
        text: `KYC-tainted UTXO mixed with P2P-acquired output — links your real-name identity to the P2P side.`,
        tone: 'amber'
      };
    }
    return {
      text: `Selection contains UTXOs with no provenance label — review before broadcasting.`,
      tone: 'amber'
    };
  }

  onMount(refresh);
</script>

<Card
  title="Coin control · UTXO labels"
  subtitle="Track UTXO provenance to avoid linking KYC-tainted coins with private ones."
>
  {#if error}
    <p class="text-sm text-rose-400">{error}</p>
  {/if}

  <section class="space-y-2">
    <h3 class="text-sm font-semibold">Labelled UTXOs</h3>
    {#if entries.length === 0}
      <p class="text-sm text-slate-400">No UTXOs labelled yet.</p>
    {:else}
      <div class="space-y-2">
        {#each entries as entry (entry.utxo.txid + ':' + entry.utxo.vout)}
          <div class="rounded border border-slate-700 p-2 space-y-1">
            <div class="font-mono text-xs text-slate-300">
              {entry.utxo.txid.slice(0, 12)}…:{entry.utxo.vout}
            </div>
            <div class="flex flex-wrap items-center gap-2">
              <select
                class="rounded bg-slate-800 px-2 py-1 text-xs"
                bind:value={entry.label.origin}
              >
                {#each ORIGINS as o}
                  <option value={o}>{originLabel(o)}</option>
                {/each}
              </select>
              <input
                class="rounded bg-slate-800 px-2 py-1 text-xs flex-1"
                placeholder="note"
                bind:value={entry.label.note}
              />
              <Button on:click={() => updateRow(entry)} disabled={busy}>Save</Button>
              <Button on:click={() => removeRow(entry)} disabled={busy}>Remove</Button>
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </section>

  <section class="space-y-2">
    <h3 class="text-sm font-semibold">Add label</h3>
    <div class="flex flex-wrap items-center gap-2">
      <input
        class="rounded bg-slate-800 px-2 py-1 text-xs flex-1 font-mono"
        placeholder="txid (hex)"
        bind:value={newTxid}
      />
      <input
        type="number"
        min="0"
        class="w-20 rounded bg-slate-800 px-2 py-1 text-xs"
        placeholder="vout"
        bind:value={newVout}
      />
      <select class="rounded bg-slate-800 px-2 py-1 text-xs" bind:value={newOrigin}>
        {#each ORIGINS as o}
          <option value={o}>{originLabel(o)}</option>
        {/each}
      </select>
      <input
        class="rounded bg-slate-800 px-2 py-1 text-xs"
        placeholder="note"
        bind:value={newNote}
      />
      <input
        class="rounded bg-slate-800 px-2 py-1 text-xs"
        placeholder="tags (comma)"
        bind:value={newTags}
      />
      <Button on:click={addEntry} disabled={busy}>Add</Button>
    </div>
  </section>

  <section class="space-y-2">
    <h3 class="text-sm font-semibold">Privacy planner</h3>
    <p class="text-xs text-slate-400">
      Paste an array of <code>LabeledUtxo</code> objects (or load the demo pool) to detect mixing problems
      and ask the wallet for a single-bucket selection.
    </p>
    <div class="flex gap-2">
      <Button on:click={loadDemoPool} disabled={busy}>Load demo pool</Button>
    </div>
    <textarea
      rows="8"
      class="w-full rounded bg-slate-900 p-2 font-mono text-xs"
      placeholder={'[\n  { "utxo": { "txid": "…", "vout": 0 }, "value": 10000, "confirmations": 6, "label": { "origin": "private", "note": "", "tags": [] } }\n]'}
      bind:value={poolJson}
    ></textarea>

    <div class="flex flex-wrap items-end gap-2">
      <label class="text-xs flex flex-col">
        <span>Target (sats)</span>
        <input
          type="number"
          min="0"
          class="w-32 rounded bg-slate-800 px-2 py-1"
          bind:value={target}
        />
      </label>
      <label class="text-xs flex flex-col">
        <span>Strategy</span>
        <select class="rounded bg-slate-800 px-2 py-1" bind:value={strategy}>
          {#each STRATEGIES as s}
            <option value={s}>{s.replace('_', ' ')}</option>
          {/each}
        </select>
      </label>
      <Button on:click={detectMix} disabled={busy}>Detect mix</Button>
      <Button on:click={suggestSelection} disabled={busy}>Suggest selection</Button>
      <Button on:click={refresh} disabled={busy}>Refresh labels</Button>
    </div>

    {#if warnings.length > 0}
      <div class="space-y-1">
        {#each warnings as w}
          {@const v = warningLabel(w)}
          <p
            class="rounded px-2 py-1 text-xs"
            class:bg-rose-900={v.tone === 'rose'}
            class:bg-amber-900={v.tone === 'amber'}
          >
            {v.text}
          </p>
        {/each}
      </div>
    {/if}

    {#if selection}
      <div class="rounded border border-emerald-700 p-2 text-xs space-y-1">
        <div>
          Bucket: <strong>{selection.bucket}</strong> · Total:
          <strong>{selection.total_value}</strong> sats ·
          <strong>{selection.inputs.length}</strong> input(s)
        </div>
        {#each selection.inputs as input}
          <div class="font-mono text-[11px] text-slate-300">
            {input.utxo.txid.slice(0, 12)}…:{input.utxo.vout} · {input.value} sats · {originLabel(
              input.label.origin
            )}
          </div>
        {/each}
      </div>
    {/if}
  </section>
</Card>
