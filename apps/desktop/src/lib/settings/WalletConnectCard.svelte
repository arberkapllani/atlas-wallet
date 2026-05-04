<script lang="ts">
  import Card from '$lib/ui/Card.svelte';
  import Button from '$lib/ui/Button.svelte';
  import { api, errorMessage, type WcUri } from '$lib/api';

  let raw = '';
  let parsed: WcUri | null = null;
  let error = '';
  let busy = false;

  async function parse() {
    if (!raw.trim()) return;
    busy = true;
    error = '';
    parsed = null;
    try {
      parsed = await api.wcParseUri(raw.trim());
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  function fmtTs(ts: number | null): string {
    if (ts === null) return '—';
    return new Date(ts * 1000).toLocaleString();
  }
</script>

<Card
  title="WalletConnect v2 URI inspector"
  subtitle="Paste a wc:… pairing URI and decode its parts before approving the session: pairing topic, relay protocol, sym-key, expiry and advertised methods. Pure offline parsing."
>
  <div class="space-y-3 text-sm">
    {#if error}
      <p class="text-rose-400 text-xs">{error}</p>
    {/if}

    <textarea
      bind:value={raw}
      rows="3"
      placeholder="wc:abcd...@2?relay-protocol=irn&symKey=..."
      class="w-full px-3 py-2 rounded-md bg-bg-elevated border border-border-subtle text-fg font-mono text-[11px]"
    ></textarea>

    <Button variant="primary" on:click={parse} disabled={busy || !raw.trim()}>
      {busy ? 'Parsing…' : 'Parse URI'}
    </Button>

    {#if parsed}
      <div class="rounded-lg border border-border-subtle p-3 space-y-1.5 text-xs">
        <p>
          <span class="text-fg-muted">Topic:</span>
          <span class="font-mono break-all">{parsed.topic}</span>
        </p>
        <p>
          <span class="text-fg-muted">Relay protocol:</span>
          <span class="font-mono">{parsed.relay_protocol}</span>
        </p>
        {#if parsed.relay_data}
          <p>
            <span class="text-fg-muted">Relay data:</span>
            <span class="font-mono">{parsed.relay_data}</span>
          </p>
        {/if}
        <p>
          <span class="text-fg-muted">Sym key:</span>
          <span class="font-mono break-all">{parsed.sym_key}</span>
        </p>
        <p>
          <span class="text-fg-muted">Expires:</span>
          <span class="font-mono">{fmtTs(parsed.expiry_timestamp)}</span>
        </p>
        {#if parsed.methods.length > 0}
          <p>
            <span class="text-fg-muted">Methods:</span>
            <span class="font-mono">{parsed.methods.join(', ')}</span>
          </p>
        {/if}
      </div>
    {/if}
  </div>
</Card>
