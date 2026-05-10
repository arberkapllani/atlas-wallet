<script lang="ts">
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import { wallet } from '$lib/stores/wallet';

  onMount(() => {
    let routed = false;
    return wallet.subscribe(($w) => {
      // Wait until the backend has reported vault/unlock status before we
      // make a routing decision — otherwise the initial `initialized:false`
      // default would always send users to /onboarding even if a vault
      // already exists on disk.
      if (!$w.statusLoaded || routed) return;
      routed = true;
      if (!$w.initialized) {
        void goto('/onboarding');
      } else if (!$w.unlocked) {
        void goto('/unlock');
      } else {
        void goto('/portfolio');
      }
    });
  });
</script>

<div class="grid place-items-center min-h-screen">
  <span class="text-fg-muted text-sm">Loading…</span>
</div>
