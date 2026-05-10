<script lang="ts">
  import { onMount } from 'svelte';
  import Card from '$lib/ui/Card.svelte';
  import Button from '$lib/ui/Button.svelte';
  import {
    api,
    errorMessage,
    type NodePolicy,
    type NodeDecision,
    type NodePolicyAuditRow
  } from '$lib/api';

  let policy: NodePolicy | null = null;
  let audit: NodePolicyAuditRow[] = [];
  let trustedHostsInput = '';
  let previewUrl = '';
  let preview: NodeDecision | null = null;
  let previewError = '';
  let error = '';
  let busy = false;

  async function refresh() {
    busy = true;
    error = '';
    try {
      policy = await api.nodePolicyGet();
      trustedHostsInput = policy.trusted_hosts.join('\n');
      audit = await api.nodePolicyAuditEndpoints();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  async function save() {
    if (!policy) return;
    busy = true;
    error = '';
    try {
      const next: NodePolicy = {
        require_local: policy.require_local,
        allow_tor_onion: policy.allow_tor_onion,
        trusted_hosts: trustedHostsInput
          .split('\n')
          .map((h) => h.trim())
          .filter((h) => h.length > 0)
      };
      policy = await api.nodePolicySet(next);
      trustedHostsInput = policy.trusted_hosts.join('\n');
      audit = await api.nodePolicyAuditEndpoints();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  async function checkPreview() {
    busy = true;
    previewError = '';
    preview = null;
    try {
      preview = await api.nodePolicyCheckUrl(previewUrl);
    } catch (e) {
      previewError = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  function decisionTone(d: NodeDecision | undefined | null): string {
    if (!d) return 'text-slate-400';
    return d.kind === 'allow' ? 'text-emerald-400' : 'text-rose-400';
  }

  function decisionLabel(d: NodeDecision | undefined | null): string {
    if (!d) return 'no decision';
    return d.kind === 'allow' ? `allow · ${d.reason}` : `block · ${d.reason}`;
  }

  onMount(refresh);
</script>

<Card
  title="Sovereign node · own-node-only mode"
  subtitle="Refuse third-party RPC providers. Atlas only dials nodes you control or reach over Tor."
>
  {#if error}
    <p class="text-sm text-rose-400">{error}</p>
  {/if}

  {#if policy}
    <section class="space-y-2">
      <label class="flex items-center gap-2 text-sm">
        <input type="checkbox" bind:checked={policy.require_local} />
        <span>
          <strong>Require local node</strong> — block any URL that is not loopback, .onion, or in the
          trusted-hosts list.
        </span>
      </label>
      <label class="flex items-center gap-2 text-sm">
        <input type="checkbox" bind:checked={policy.allow_tor_onion} />
        <span>
          Allow <code>*.onion</code> hosts (recommended — reached through your own Tor circuit).
        </span>
      </label>
      <label class="flex flex-col text-sm">
        <span class="font-semibold">Trusted hosts (one per line)</span>
        <textarea
          rows="4"
          class="rounded bg-slate-900 p-2 font-mono text-xs"
          placeholder={'my-node.lan\n10.0.0.5\nfriends-node.local'}
          bind:value={trustedHostsInput}
        ></textarea>
      </label>
      <div class="flex gap-2">
        <Button on:click={save} disabled={busy}>Save policy</Button>
        <Button on:click={refresh} disabled={busy}>Refresh</Button>
      </div>
    </section>

    <section class="space-y-2">
      <h3 class="text-sm font-semibold">Preview decision</h3>
      <div class="flex gap-2">
        <input
          class="flex-1 rounded bg-slate-800 px-2 py-1 text-xs font-mono"
          placeholder="https://mainnet.infura.io/v3/..."
          bind:value={previewUrl}
        />
        <Button on:click={checkPreview} disabled={busy || !previewUrl.trim()}>Check</Button>
      </div>
      {#if previewError}
        <p class="text-xs text-rose-400">{previewError}</p>
      {:else if preview}
        <p class="text-xs {decisionTone(preview)}">
          {preview.host} → {decisionLabel(preview)}
        </p>
      {/if}
    </section>

    <section class="space-y-2">
      <h3 class="text-sm font-semibold">Currently configured RPCs</h3>
      {#if audit.length === 0}
        <p class="text-xs text-slate-400">No chains configured.</p>
      {:else}
        <div class="space-y-1">
          {#each audit as row}
            <div class="flex items-baseline gap-2 text-xs">
              <span class="w-28 font-mono">{row.chain_id}</span>
              <span class="flex-1 truncate font-mono text-slate-400">
                {row.url ?? '(none)'}
              </span>
              <span class={decisionTone(row.decision)}>
                {decisionLabel(row.decision)}
              </span>
            </div>
          {/each}
        </div>
      {/if}
    </section>
  {/if}
</Card>
