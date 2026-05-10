<script lang="ts">
  export let title: string | undefined = undefined;
  export let subtitle: string | undefined = undefined;
  export let padded = true;
  /** Adds a soft brand glow at the top — use sparingly for hero cards. */
  export let glow = false;
</script>

<section
  class="rounded-2xl bg-bg-subtle border border-border-subtle shadow-card overflow-hidden relative"
  class:bg-brand-soft={glow}
>
  {#if title}
    <header class="px-6 pt-5 pb-3 border-b border-border-subtle">
      <h2 class="text-base font-semibold tracking-tight text-fg">{title}</h2>
      {#if subtitle}
        <p class="text-sm text-fg-muted mt-1">{subtitle}</p>
      {/if}
    </header>
  {/if}
  <div class={padded ? 'p-6' : ''}>
    <slot />
  </div>
</section>
