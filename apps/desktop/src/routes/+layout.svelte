<script lang="ts">
  import '../app.css';
  import { onMount } from 'svelte';
  import { wallet } from '$lib/stores/wallet';

  onMount(() => {
    void wallet.refreshStatus();

    const onActivity = () => wallet.ping();
    window.addEventListener('mousemove', onActivity, { passive: true });
    window.addEventListener('keydown', onActivity);
    return () => {
      window.removeEventListener('mousemove', onActivity);
      window.removeEventListener('keydown', onActivity);
    };
  });
</script>

<main class="min-h-screen bg-bg text-fg">
  <slot />
</main>
