<script lang="ts">
  import { onMount } from 'svelte';
  import Card from '$lib/ui/Card.svelte';
  import Button from '$lib/ui/Button.svelte';
  import { api, errorMessage, type BiometricStatus } from '$lib/api';

  let status: BiometricStatus | null = null;
  let error = '';
  let busy = false;

  async function refresh() {
    busy = true;
    error = '';
    try {
      status = await api.biometricStatusReport();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  onMount(refresh);
</script>

<Card
  title="Biometric unlock status"
  subtitle="Reports whether the OS exposes an enrolled biometric sensor (Touch ID / Windows Hello / fingerprint) and whether biometric unlock is currently enabled in the wallet's encrypted settings."
>
  <div class="space-y-3 text-sm">
    {#if error}
      <p class="text-rose-400 text-xs">{error}</p>
    {/if}

    {#if status}
      <ul class="text-xs space-y-1">
        <li>
          <span class="text-fg-muted">Sensor available:</span>
          <span class="font-mono {status.available ? 'text-emerald-400' : 'text-rose-400'}">
            {status.available ? 'yes' : 'no'}
          </span>
        </li>
        <li>
          <span class="text-fg-muted">Enabled in settings:</span>
          <span class="font-mono {status.enabled ? 'text-emerald-400' : 'text-fg-subtle'}">
            {status.enabled ? 'yes' : 'no'}
          </span>
        </li>
      </ul>
      {#if !status.available}
        <p class="text-[11px] text-fg-subtle">
          No enrolled sensor detected. Enable Touch ID / Windows Hello in your OS first.
        </p>
      {/if}
    {/if}

    <Button variant="secondary" on:click={refresh} disabled={busy}>
      {busy ? 'Refreshing…' : 'Refresh'}
    </Button>

    <p class="text-[11px] text-fg-subtle">
      Toggling the preference itself happens in the existing Security card; this view exists so you
      can verify that the OS sensor is reachable before enabling biometric unlock.
    </p>
  </div>
</Card>
