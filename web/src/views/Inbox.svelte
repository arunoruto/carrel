<script lang="ts">
  import { Check, EyeOff, FileText, Inbox as InboxIcon, RefreshCw, Sparkles } from '@lucide/svelte';
  import { api, type Inbox, type InboxFile } from '../lib/api';
  import Badge from '../lib/components/Badge.svelte';
  import Button from '../lib/components/Button.svelte';
  import Spinner from '../lib/components/Spinner.svelte';
  import { ago, bytes } from '../lib/markdown';
  import { paperHref } from '../lib/router.svelte';
  import { app } from '../lib/state.svelte';
  import { toasts } from '../lib/toast.svelte';

  // `carrel consume` in the browser: the PDFs in your Downloads, to move into the library or leave.
  let inbox = $state<Inbox | null>(null);
  let error = $state('');
  let busy = $state<Set<string>>(new Set());
  let showSkipped = $state(false);

  async function load() {
    try {
      inbox = await api.inbox();
      error = '';
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }
  load();

  async function act(f: InboxFile, add: boolean) {
    busy = new Set(busy).add(f.path);
    try {
      if (add) {
        const { id, messages } = await api.consume(f.path);
        toasts.show('success', `Added ${id}`, [`from ${f.name}`, ...messages].join('\n'));
      } else {
        await api.skip(f.path);
      }
      await load();
      app.changed();
    } catch (e) {
      toasts.error(e, `Cannot add ${f.name}`);
    }
    const next = new Set(busy);
    next.delete(f.path);
    busy = next;
  }

  const waiting = $derived(inbox?.files.filter((f) => !f.skipped) ?? []);
  const skipped = $derived(inbox?.files.filter((f) => f.skipped) ?? []);
  const likely = $derived(waiting.filter((f) => f.clue && f.clue !== 'mentions arXiv'));
</script>

<div class="mx-auto flex max-w-4xl flex-col gap-5 px-6 py-6 lg:px-10 lg:py-8">
  <header class="flex flex-wrap items-end justify-between gap-3">
    <div>
      <h1 class="text-2xl font-semibold tracking-tight">Inbox</h1>
      <p class="mt-1 text-sm text-muted">
        PDFs from the last {inbox?.days ?? 30} days in {inbox?.folders.join(', ') ?? 'your Downloads'}. Added ones move into the library.
      </p>
    </div>
    <div class="flex gap-2">
      <Button variant="ghost" onclick={load}><RefreshCw class="size-4" />Refresh</Button>
      {#if likely.length > 1}
        <Button variant="primary" onclick={() => likely.forEach((f) => act(f, true))}><Sparkles class="size-4" />Add the {likely.length} papers</Button>
      {/if}
    </div>
  </header>

  {#if error}<div class="rounded-xl border border-failed/40 bg-failed/10 px-4 py-3 text-sm text-failed">{error}</div>{/if}

  {#if !inbox && !error}
    <div class="flex justify-center py-16"><Spinner class="size-6 text-faint" /></div>
  {:else if inbox}
    <div class="overflow-hidden rounded-xl border bg-card shadow-sm">
      {#each waiting as f (f.path)}
        <div class="flex items-center gap-4 border-b px-5 py-3.5 last:border-b-0">
          <div class="flex size-9 shrink-0 items-center justify-center rounded-lg {f.clue ? 'bg-accent/10 text-accent' : 'bg-hover text-faint'}">
            <FileText class="size-4.5" />
          </div>
          <div class="min-w-0 flex-1">
            <div class="truncate text-sm font-medium" title={f.path}>{f.name}</div>
            <div class="mt-0.5 flex flex-wrap items-center gap-x-2 text-xs text-muted">
              <span>{bytes(f.size)}</span><span class="text-faint">·</span><span>{ago(f.modified)}</span>
              {#if f.clue}<Badge tone={f.clue === 'mentions arXiv' ? 'neutral' : 'accent'}>{f.clue}</Badge>{:else}<span class="text-faint">· no DOI or arXiv id found</span>{/if}
            </div>
          </div>
          {#if busy.has(f.path)}
            <div class="flex items-center gap-2 text-xs text-muted"><Spinner />Adding…</div>
          {:else}
            <Button size="sm" variant="ghost" onclick={() => act(f, false)} title="Leave it, and don't offer it again"><EyeOff class="size-3.5" />Skip</Button>
            <Button size="sm" variant={f.clue ? 'primary' : 'outline'} onclick={() => act(f, true)}><Check class="size-3.5" />Add</Button>
          {/if}
        </div>
      {:else}
        <div class="flex flex-col items-center gap-3 px-6 py-16 text-center">
          <InboxIcon class="size-10 text-faint" />
          <div class="font-medium">Nothing new</div>
          <p class="max-w-sm text-sm text-muted">New PDFs in your Downloads show up here.</p>
        </div>
      {/each}
    </div>

    {#if skipped.length}
      <button class="cursor-pointer self-start text-sm text-muted hover:text-foreground" onclick={() => (showSkipped = !showSkipped)}>
        {showSkipped ? 'Hide' : 'Show'} {skipped.length} skipped
      </button>
      {#if showSkipped}
        <div class="overflow-hidden rounded-xl border bg-card/60">
          {#each skipped as f (f.path)}
            <div class="flex items-center gap-4 border-b px-5 py-2.5 text-sm text-muted last:border-b-0">
              <span class="min-w-0 flex-1 truncate">{f.name}</span>
              <Button size="sm" variant="ghost" busy={busy.has(f.path)} onclick={() => act(f, true)}>Add anyway</Button>
            </div>
          {/each}
        </div>
      {/if}
    {/if}
  {/if}
</div>
