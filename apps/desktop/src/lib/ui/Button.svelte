<script lang="ts">
  /** Visual variant. */
  export let variant: 'primary' | 'secondary' | 'ghost' | 'danger' | 'accent' = 'primary';
  export let size: 'sm' | 'md' | 'lg' = 'md';
  export let type: 'button' | 'submit' | 'reset' = 'button';
  export let disabled = false;
  export let loading = false;
  export let fullWidth = false;

  const variants: Record<string, string> = {
    primary:
      'bg-brand-600 hover:bg-brand-500 text-white shadow-glow disabled:opacity-50 disabled:shadow-none',
    accent:
      'bg-accent hover:bg-accent-hover text-white shadow-accentGlow disabled:opacity-50 disabled:shadow-none',
    secondary:
      'bg-bg-elevated hover:bg-border text-fg border border-border disabled:opacity-50',
    ghost:
      'bg-transparent hover:bg-bg-elevated text-fg-muted hover:text-fg',
    danger: 'bg-danger/90 hover:bg-danger text-white'
  };
  const sizes: Record<string, string> = {
    sm: 'h-8 px-3 text-sm',
    md: 'h-10 px-4 text-sm',
    lg: 'h-12 px-6 text-base'
  };
</script>

<button
  {type}
  class="inline-flex items-center justify-center gap-2 rounded-xl font-semibold tracking-tight transition-all duration-200 ease-smooth active:scale-[0.98] disabled:cursor-not-allowed {variants[
    variant
  ]} {sizes[size]} {fullWidth ? 'w-full' : ''}"
  disabled={disabled || loading}
  on:click
>
  {#if loading}
    <span
      class="h-4 w-4 animate-spin rounded-full border-2 border-current border-t-transparent"
      aria-hidden="true"
    ></span>
  {/if}
  <slot />
</button>
