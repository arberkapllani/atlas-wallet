<script lang="ts">
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import { wallet } from '$lib/stores/wallet';

  onMount(() => {
    return wallet.subscribe(($w) => {
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
