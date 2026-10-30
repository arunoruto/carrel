<script lang="ts">
  import { ArrowUpRight, Plus, RefreshCw } from '@lucide/svelte';
  import { addSource } from '../lib/actions';
  import { api, type Reference } from '../lib/api';
  import Badge from '../lib/components/Badge.svelte';
  import Button from '../lib/components/Button.svelte';
  import Spinner from '../lib/components/Spinner.svelte';
  import { byline } from '../lib/markdown';
  import { paperHref } from '../lib/router.svelte';

  // What a paper cites (looked up once, then kept in references.json), or who cites it (asked online).
  let { id, citedBy = false }: { id: string; citedBy?: boolean } = $props();
  let refs = $state<Reference[] | null>(null);
  let error = $state('');
  let adding = $state<string | null>(null);
  let filter = $state<'all' | 'in' | 'out'>('all');

  async function load(refresh = false) {
    refs = null;
    try {
      refs = citedBy ? await api.citedBy(id) : await api.references(id, refresh);
      error = '';
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      refs = [];
    }
  }

  $effect(() => {
    void [id, citedBy];
    load();
  });

  const source = (r: Reference) => r.add_with ?? (r.doi ? r.doi : r.arxiv ? `arXiv:${r.arxiv}` : r.title);

  async function add(r: Reference) {
    adding = r.title;
    const newId = await addSource(source(r), false);
    if (newId) r.in_library = newId;
    adding = null;
  }

  const shown = $derived((refs ?? []).filter((r) => filter === 'all' || (filter === 'in') === !!r.in_library));
  const inLibrary = $derived((refs ?? []).filter((r) => r.in_library).length);
</script>

{#if refs === null}
  <div class="flex flex-col items-center gap-2 py-10 text-sm text-muted">
    <Spinner class="size-6 text-faint" />{citedBy ? 'Asking who cites it…' : 'Loading references…'}
  </div>
{:else}
  <div class="mb-4 flex flex-wrap items-center gap-2">
    <div class="flex rounded-lg border bg-card p-0.5 text-xs">
      {#each [['all', `All ${refs.length}`], ['in', `In library ${inLibrary}`], ['out', `Missing ${refs.length - inLibrary}`]] as [key, label]}
        <button class="cursor-pointer rounded-md px-2.5 py-1 {filter === key ? 'bg-hover font-medium' : 'text-muted'}" onclick={() => (filter = key as typeof filter)}>{label}</button>
      {/each}
    </div>
    {#if !citedBy}
      <Button size="sm" variant="ghost" class="ml-auto" onclick={() => load(true)} title="Look the references up again"><RefreshCw class="size-3.5" />Look up again</Button>
    {/if}
  </div>
  {#if error}<p class="mb-3 text-sm text-failed">{error}</p>{/if}
  <ul class="space-y-1">
    {#each shown as r}
      <li class="group flex items-start gap-3 rounded-lg px-3 py-2 hover:bg-hover/50">
        <div class="min-w-0 flex-1">
          {#if r.in_library}
            <a href={paperHref(r.in_library)} class="text-sm font-medium text-accent hover:underline">{r.title}</a>
          {:else}
            <div class="text-sm">{r.title}</div>
          {/if}
          <div class="mt-0.5 text-xs text-muted">
            {byline(r)}
            {#if r.source === 'ai'}<Badge tone="warning">from AI</Badge>{/if}
          </div>
        </div>
        {#if r.in_library}
          <a href={paperHref(r.in_library)} class="shrink-0 text-faint hover:text-accent" aria-label="Open"><ArrowUpRight class="size-4" /></a>
        {:else if r.doi || r.arxiv || r.add_with}
          <Button size="sm" class="opacity-0 group-hover:opacity-100 focus:opacity-100" busy={adding === r.title} onclick={() => add(r)}><Plus class="size-3.5" />Add</Button>
        {/if}
      </li>
    {:else}
      <p class="py-6 text-center text-sm text-muted">{citedBy ? 'No source knows papers citing it.' : 'No references known for it.'}</p>
    {/each}
  </ul>
{/if}
