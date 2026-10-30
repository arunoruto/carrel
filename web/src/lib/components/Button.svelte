<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { HTMLButtonAttributes } from 'svelte/elements';
  import Spinner from './Spinner.svelte';

  let {
    variant = 'outline',
    size = 'md',
    busy = false,
    class: extra = '',
    children,
    ...rest
  }: HTMLButtonAttributes & {
    variant?: 'primary' | 'outline' | 'ghost' | 'danger';
    size?: 'sm' | 'md' | 'icon';
    busy?: boolean;
    children?: Snippet;
  } = $props();

  const variants = {
    primary: 'bg-accent text-accent-foreground hover:brightness-110 shadow-sm',
    outline: 'border bg-card hover:bg-hover',
    ghost: 'hover:bg-hover',
    danger: 'bg-failed text-white hover:brightness-110',
  };
  const sizes = { sm: 'h-7 px-2.5 text-xs gap-1.5', md: 'h-9 px-3.5 text-sm gap-2', icon: 'h-8 w-8 justify-center' };
</script>

<button
  class="inline-flex shrink-0 cursor-pointer items-center rounded-lg font-medium whitespace-nowrap transition focus-visible:ring-2 focus-visible:ring-accent/50 focus-visible:outline-none disabled:cursor-default disabled:opacity-50 {variants[variant]} {sizes[size]} {extra}"
  disabled={busy || rest.disabled}
  {...rest}
>
  {#if busy}<Spinner />{/if}
  {@render children?.()}
</button>
