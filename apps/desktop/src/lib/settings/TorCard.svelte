<script lang="ts">
  import { onMount } from 'svelte';
  import Card from '$lib/ui/Card.svelte';
  import Button from '$lib/ui/Button.svelte';
  import {
    api,
    errorMessage,
    type TorMode,
    type TorStatus,
    type TorConfig,
    type ProxyDecision
  } from '$lib/api';

  let mode: TorMode = 'required';
  let status: TorStatus | null = null;
  let config: TorConfig | null = null;
  let decision: ProxyDecision | null = null;
  let error = '';
  let busy = false;

  // Editable fields for config (pushed via Save).
  let socksAddrInput = '';
  let bridgesInput = '';

  async function refresh() {
    busy = true;
    error = '';
    try {
      [mode, status, config, decision] = await Promise.all([
        api.torGetMode(),
        api.torStatus(),
        api.torGetConfig(),
        api.torEnforceDecision()
      ]);
      socksAddrInput = config.socks_addr;
      bridgesInput = config.bridges.join('\n');
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  async function setMode(next: TorMode) {
    busy = true;
    error = '';
    try {
      mode = await api.torSetMode(next);
      decision = await api.torEnforceDecision();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  async function start() {
    busy = true;
    error = '';
    try {
      status = await api.torStart();
      decision = await api.torEnforceDecision();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  async function stop() {
    busy = true;
    error = '';
    try {
      await api.torStop();
      status = await api.torStatus();
      decision = await api.torEnforceDecision();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  async function newCircuit() {
    busy = true;
    error = '';
    try {
      await api.torNewCircuit();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  async function saveConfig() {
    busy = true;
    error = '';
    try {
      const bridges = bridgesInput
        .split('\n')
        .map((l) => l.trim())
        .filter((l) => l.length > 0);
      config = await api.torSetConfig({ socks_addr: socksAddrInput.trim(), bridges });
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  function statusLabel(s: TorStatus | null): string {
    if (!s) return '—';
    switch (s.kind) {
      case 'disabled':
        return 'Stopped';
      case 'bootstrapping':
        return `Bootstrapping (${s.progress}%)`;
      case 'ready':
        return 'Ready';
      case 'failed':
        return `Failed: ${s.reason}`;
    }
  }

  function statusTone(s: TorStatus | null): string {
    if (!s) return 'text-fg-subtle';
    switch (s.kind) {
      case 'ready':
        return 'text-emerald-400';
      case 'bootstrapping':
        return 'text-amber-400';
      case 'failed':
        return 'text-rose-400';
      default:
        return 'text-fg-subtle';
    }
  }

  function decisionLabel(d: ProxyDecision | null): string {
    if (!d) return '—';
    switch (d.kind) {
      case 'direct':
        return 'Direct (clearnet)';
      case 'socks5':
        return `SOCKS5 → ${d.addr}`;
      case 'block':
        return `Blocked: ${d.reason}`;
    }
  }

  function decisionTone(d: ProxyDecision | null): string {
    if (!d) return 'text-fg-subtle';
    switch (d.kind) {
      case 'socks5':
        return 'text-emerald-400';
      case 'block':
        return 'text-rose-400';
      case 'direct':
        return 'text-amber-400';
    }
  }

  onMount(refresh);
</script>

<Card
  title="Tor proxy & kill-switch"
  subtitle="Routes every outbound network request through Tor. With mode = Required, requests are blocked rather than leaking your IP onto clearnet when Tor is unavailable. The default for new installs is Required."
>
  <div class="space-y-4 text-sm">
    {#if error}
      <p class="text-rose-400 text-xs">{error}</p>
    {/if}

    <!-- Status row -->
    <div class="grid grid-cols-2 gap-2 text-xs">
      <div>
        <span class="text-fg-muted">Status:</span>
        <span class="font-mono {statusTone(status)}">{statusLabel(status)}</span>
      </div>
      <div>
        <span class="text-fg-muted">Next request:</span>
        <span class="font-mono {decisionTone(decision)}">{decisionLabel(decision)}</span>
      </div>
    </div>

    <!-- Mode selector -->
    <fieldset class="space-y-2">
      <legend class="text-xs text-fg-muted">Posture</legend>
      <label class="flex items-start gap-2 text-xs">
        <input
          type="radio"
          name="tor-mode"
          checked={mode === 'required'}
          on:change={() => setMode('required')}
          disabled={busy}
        />
        <span>
          <span class="font-medium text-emerald-400">Required</span> —
          <span class="text-fg-subtle"
            >use Tor or block the request. Maximum privacy. Recommended.</span
          >
        </span>
      </label>
      <label class="flex items-start gap-2 text-xs">
        <input
          type="radio"
          name="tor-mode"
          checked={mode === 'preferred'}
          on:change={() => setMode('preferred')}
          disabled={busy}
        />
        <span>
          <span class="font-medium text-amber-400">Preferred</span> —
          <span class="text-fg-subtle"
            >use Tor when available, fall back to clearnet. Leaks IP on failure.</span
          >
        </span>
      </label>
      <label class="flex items-start gap-2 text-xs">
        <input
          type="radio"
          name="tor-mode"
          checked={mode === 'disabled'}
          on:change={() => setMode('disabled')}
          disabled={busy}
        />
        <span>
          <span class="font-medium text-rose-400">Disabled</span> —
          <span class="text-fg-subtle"
            >direct clearnet for every request. Only safe behind a Whonix/Tails gateway.</span
          >
        </span>
      </label>
    </fieldset>

    <!-- Lifecycle -->
    <div class="flex flex-wrap gap-2">
      <Button variant="secondary" on:click={start} disabled={busy}>Start</Button>
      <Button variant="secondary" on:click={stop} disabled={busy}>Stop</Button>
      <Button
        variant="secondary"
        on:click={newCircuit}
        disabled={busy || status?.kind !== 'ready'}>New circuit</Button
      >
      <Button variant="secondary" on:click={refresh} disabled={busy}>Refresh</Button>
    </div>

    <!-- Config -->
    <details class="text-xs">
      <summary class="cursor-pointer text-fg-muted">Advanced — SOCKS address & bridges</summary>
      <div class="mt-2 space-y-2">
        <label class="block">
          <span class="text-fg-muted">SOCKS5 listener (host:port)</span>
          <input
            class="mt-1 w-full rounded bg-bg-subtle px-2 py-1 font-mono"
            bind:value={socksAddrInput}
            placeholder="127.0.0.1:9050"
            disabled={busy}
          />
        </label>
        <label class="block">
          <span class="text-fg-muted">Bridges (one per line)</span>
          <textarea
            class="mt-1 w-full rounded bg-bg-subtle px-2 py-1 font-mono"
            rows="3"
            bind:value={bridgesInput}
            placeholder="obfs4 192.0.2.1:443 ABCDEF…"
            disabled={busy}
          ></textarea>
        </label>
        <Button variant="secondary" on:click={saveConfig} disabled={busy}>Save config</Button>
      </div>
    </details>

    <p class="text-[11px] text-fg-subtle">
      The current build ships a deterministic stub Tor provider so the kill-switch policy is
      testable today. The real <code>arti-client</code>-backed provider lands in a follow-up commit;
      it will route real traffic without changing this UI.
    </p>
  </div>
</Card>
