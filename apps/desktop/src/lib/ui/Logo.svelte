<script lang="ts">
  /** Visual size of the logo mark. */
  export let size: 'sm' | 'md' | 'lg' = 'md';
  /** Show the wordmark next to the leaf. */
  export let showWordmark = true;
  /** Subtitle line under the wordmark. */
  export let tagline: string | undefined = 'Sovereign wallet';

  const markSize: Record<string, string> = {
    sm: 'h-7 w-7',
    md: 'h-9 w-9',
    lg: 'h-12 w-12'
  };
  const wordSize: Record<string, string> = {
    sm: 'text-base',
    md: 'text-lg',
    lg: 'text-2xl'
  };
</script>

<div class="flex items-center gap-3">
  <div
    class="{markSize[size]} rounded-xl bg-brand-gradient grid place-items-center shadow-glow ring-1 ring-white/10"
  >
    <svg viewBox="0 0 24 24" class="h-3/5 w-3/5 text-white" aria-hidden="true">
      <path
        fill="currentColor"
        d="M12 2.5c-3.5 3.4-7 6.6-7 11.2 0 4.4 3.3 7.8 7 7.8s7-3.4 7-7.8c0-4.6-3.5-7.8-7-11.2Zm0 4.6c2.2 2.4 4.2 4.7 4.2 7.6 0 2.2-1.7 4.4-4.2 4.4-.1-3.4-1.7-6.5-4-9.1.7-1 1.5-2 2.8-2.9Z"
      />
    </svg>
  </div>
  {#if showWordmark}
    <div class="leading-tight">
      <h1 class="font-display font-bold tracking-tight {wordSize[size]}">
        Green<span class="text-accent">Wallet</span>
      </h1>
      {#if tagline}
        <p class="text-[10px] text-fg-subtle uppercase tracking-[0.18em] mt-1">{tagline}</p>
      {/if}
    </div>
  {/if}
</div>