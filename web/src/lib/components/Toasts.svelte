<script lang="ts">
  import { CircleAlert, CircleCheck, Info, X } from '@lucide/svelte';
  import { toasts } from '../toast.svelte';
</script>

<div class="pointer-events-none fixed right-4 bottom-4 z-50 flex w-96 max-w-[calc(100%-2rem)] flex-col gap-2">
  {#each toasts.list as t (t.id)}
    <div class="pointer-events-auto flex gap-3 rounded-xl border bg-card p-3.5 shadow-lg">
      {#if t.kind === 'error'}<CircleAlert class="size-5 shrink-0 text-failed" />
      {:else if t.kind === 'success'}<CircleCheck class="size-5 shrink-0 text-ready" />
      {:else}<Info class="size-5 shrink-0 text-accent" />{/if}
      <div class="min-w-0 flex-1">
        <div class="text-sm font-medium">{t.title}</div>
        {#if t.body}<div class="mt-0.5 text-xs break-words whitespace-pre-line text-muted">{t.body}</div>{/if}
      </div>
      <button class="h-fit cursor-pointer text-faint hover:text-foreground" onclick={() => toasts.dismiss(t.id)} aria-label="Dismiss">
        <X class="size-4" />
      </button>
    </div>
  {/each}
</div>
