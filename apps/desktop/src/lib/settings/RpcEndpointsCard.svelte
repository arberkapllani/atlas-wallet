<script lang="ts">
  import Card from '$lib/ui/Card.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Input from '$lib/ui/Input.svelte';
  import { api, type RpcEndpoint } from '$lib/api';
  import { onMount } from 'svelte';

  let endpoints: RpcEndpoint[] = [];
  let loading = true;
  let error: string | null = null;

  // Per-row local state (id -> draft url + saving flag).
  let drafts: Record<string, string> = {};
  let savingId: string | null = null;
  let rowError: Record<string, string> = {};

  async function refresh() {
    try {
      endpoints = await api.listRpcEndpoints();
      drafts = {};
      rowError = {};
      error = null;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  onMount(refresh);

  function draftFor(ep: RpcEndpoint): string {
    return drafts[ep.chain_id] ?? ep.override_url ?? '';
  }

  async function save(ep: RpcEndpoint) {
    const url = (drafts[ep.chain_id] ?? '').trim();
    if (!url) {
      rowError[ep.chain_id] = 'enter a URL or click Reset to use the default';
      rowError = rowError;
      return;
    }
    savingId = ep.chain_id;
    try {
      await api.setRpcEndpoint(ep.chain_id, url);
      delete rowError[ep.chain_id];
      await refresh();
    } catch (e) {
      rowError[ep.chain_id] = e instanceof Error ? e.message : String(e);
      rowError = rowError;
    } finally {
      savingId = null;
    }
  }

  async function reset(ep: RpcEndpoint) {
    savingId = ep.chain_id;
    try {
      await api.clearRpcEndpoint(ep.chain_id);
      delete rowError[ep.chain_id];
      await refresh();
    } catch (e) {
      rowError[ep.chain_id] = e instanceof Error ? e.message : String(e);
      rowError = rowError;
    } finally {
      savingId = null;
    }
  }
</script>

<Card
  title="Network endpoints"
  subtitle="Atlas talks to each chain through these URLs. Point them at your own node — full node, light client, or any endpoint you trust — and you stop depending on third parties."
>
  {#if loading}
    <p class="text-sm text-fg-muted">Loading…</p>
  {:else if error}
    <p class="text-sm text-danger">{error}</p>
  {:else}
    <div class="divide-y divide-border-subtle -mx-6 -my-2">
      {#each endpoints as ep (ep.chain_id)}
        <div class="px-6 py-4 space-y-2">
          <div class="flex items-baseline justify-between gap-3">
            <div>
              <span class="font-mono text-sm uppercase">{ep.chain_id}</span>
              {#if ep.override_url}
                <span
                  class="ml-2 inline-block text-[10px] uppercase tracking-wider rounded bg-accent/15 text-accent px-1.5 py-0.5"
                  >custom</span
                >
              {:else}
                <span
                  class="ml-2 inline-block text-[10px] uppercase tracking-wider rounded bg-bg-elevated text-fg-subtle px-1.5 py-0.5"
                  >default</span
                >
              {/if}
            </div>
            <span class="text-xs text-fg-subtle truncate max-w-[60%]" title={ep.effective_url ?? ''}
              >{ep.effective_url ?? '—'}</span
            >
          </div>

          <Input
            value={draftFor(ep)}
            on:input={(e) => {
              drafts[ep.chain_id] = (e.target as HTMLInputElement).value;
              drafts = drafts;
            }}
            placeholder={ep.default_url ?? 'https://your-node.example/rpc'}
            hint={ep.default_url ? `default: ${ep.default_url}` : undefined}
            error={rowError[ep.chain_id]}
          />

          <div class="flex gap-2">
            <Button
              size="sm"
              loading={savingId === ep.chain_id}
              disabled={savingId !== null && savingId !== ep.chain_id}
              on:click={() => save(ep)}
            >
              Save
            </Button>
            {#if ep.override_url}
              <Button
                size="sm"
                variant="secondary"
                disabled={savingId !== null}
                on:click={() => reset(ep)}
              >
                Reset to default
              </Button>
            {/if}
          </div>
        </div>
      {/each}
    </div>
  {/if}
</Card>
