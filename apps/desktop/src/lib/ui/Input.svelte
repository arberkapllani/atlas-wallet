<script lang="ts">
  export let value = '';
  export let placeholder = '';
  export let label: string | undefined = undefined;
  export let type: 'text' | 'password' | 'email' | 'number' = 'text';
  export let id: string | undefined = undefined;
  export let autocomplete: import('svelte/elements').FullAutoFill = 'off';
  export let error: string | undefined = undefined;
  export let hint: string | undefined = undefined;
  export let disabled = false;
  export let multiline = false;
  export let rows = 3;
</script>

<label class="block">
  {#if label}
    <span class="text-sm text-fg-muted block mb-2">{label}</span>
  {/if}
  {#if multiline}
    <textarea
      {id}
      bind:value
      {placeholder}
      {disabled}
      {autocomplete}
      {rows}
      class="w-full bg-bg-elevated border border-border rounded-xl px-3.5 py-2.5
             text-fg placeholder:text-fg-subtle font-mono text-sm
             focus:outline-none focus:border-accent focus:ring-2 focus:ring-accent-ring
             disabled:opacity-50 resize-none transition"
    ></textarea>
  {:else}
    <input
      {id}
      {type}
      bind:value
      {placeholder}
      {disabled}
      {autocomplete}
      class="w-full bg-bg-elevated border border-border rounded-xl px-3.5 py-2.5
             text-fg placeholder:text-fg-subtle text-sm
             focus:outline-none focus:border-accent focus:ring-2 focus:ring-accent-ring
             disabled:opacity-50 transition"
    />
  {/if}
  {#if error}
    <span class="text-xs text-danger block mt-1.5">{error}</span>
  {:else if hint}
    <span class="text-xs text-fg-subtle block mt-1.5">{hint}</span>
  {/if}
</label>
