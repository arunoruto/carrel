<script lang="ts">
  import { X } from '@lucide/svelte';
  import type { Snippet } from 'svelte';

  let {
    open = $bindable(false),
    title,
    wide = false,
    children,
    footer,
  }: { open: boolean; title: string; wide?: boolean; children: Snippet; footer?: Snippet } = $props();

  let dialog: HTMLDialogElement;
  $effect(() => {
    if (open && !dialog.open) dialog.showModal();
    if (!open && dialog.open) dialog.close();
  });
</script>

<dialog
  bind:this={dialog}
  onclose={() => (open = false)}
  onclick={(e) => e.target === dialog && (open = false)}
  class="m-auto w-[calc(100%-2rem)] {wide ? 'max-w-2xl' : 'max-w-md'} rounded-xl border bg-card p-0 text-foreground shadow-2xl backdrop:bg-black/40 backdrop:backdrop-blur-[2px]"
>
  {#if open}
    <div class="flex items-center justify-between border-b px-5 py-3.5">
      <h2 class="font-semibold">{title}</h2>
      <button class="cursor-pointer rounded-md p-1 text-muted hover:bg-hover" onclick={() => (open = false)} aria-label="Close">
        <X class="size-4" />
      </button>
    </div>
    <div class="px-5 py-4">{@render children()}</div>
    {#if footer}<div class="flex justify-end gap-2 border-t px-5 py-3">{@render footer()}</div>{/if}
  {/if}
</dialog>
