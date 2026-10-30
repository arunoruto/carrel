<script lang="ts">
  import { Compass, Plus } from '@lucide/svelte';
  import { addSource } from '../lib/actions';
  import { api, type Reference } from '../lib/api';
  import Button from '../lib/components/Button.svelte';
  import Spinner from '../lib/components/Spinner.svelte';
  import { byline } from '../lib/markdown';
  import { paperHref } from '../lib/router.svelte';
  import { app } from '../lib/state.svelte';

  // The papers the library (or a project) cites most but doesn't have.
  let project = $state('');
  let list = $state<Reference[] | null>(null);
  let error = $state('');
  let adding = $state<string | null>(null);

  $effect(() => {
    const p = project;
    list = null;
    api
      .suggestions(p || undefined)
      .then((s) => ((list = s), (error = '')))
      .catch((e) => ((error = e.message), (list = [])));
  });

  async function add(r: Reference) {
    adding = r.title;
    const id = await addSource(r.doi ?? (r.arxiv ? `arXiv:${r.arxiv}` : r.title), false);
    if (id && list) list = list.filter((x) => x !== r);
    adding = null;
  }
</script>

<div class="mx-auto flex max-w-4xl flex-col gap-5 px-6 py-6 lg:px-10 lg:py-8">
  <header class="flex flex-wrap items-end justify-between gap-3">
    <div>
      <h1 class="text-2xl font-semibold tracking-tight">Discover</h1>
      <p class="mt-1 text-sm text-muted">What your papers cite most, but your library lacks.</p>
    </div>
    <select bind:value={project} class="h-9 cursor-pointer rounded-lg border bg-card px-3 text-sm shadow-sm outline-none">
      <option value="">Whole library</option>
      {#each app.projects as p}<option value={p.name}>{p.name}</option>{/each}
    </select>
  </header>

  {#if error}<div class="rounded-xl border border-failed/40 bg-failed/10 px-4 py-3 text-sm text-failed">{error}</div>{/if}

  {#if list === null}
    <div class="flex justify-center py-16"><Spinner class="size-6 text-faint" /></div>
  {:else}
    <div class="overflow-hidden rounded-xl border bg-card shadow-sm">
      {#each list as r, i}
        <div class="flex items-start gap-4 border-b px-5 py-3.5 last:border-b-0">
          <div class="w-6 shrink-0 pt-0.5 text-right text-sm font-semibold text-faint tabular-nums">{i + 1}</div>
          <div class="min-w-0 flex-1">
            <div class="text-sm font-medium">{r.title}</div>
            <div class="mt-0.5 text-xs text-muted">{byline(r)}</div>
            <div class="mt-1.5 flex flex-wrap items-center gap-1 text-xs text-muted">
              cited by
              {#each (r.cited_by ?? []).slice(0, 5) as id}<a href={paperHref(id)} class="rounded bg-hover px-1.5 py-0.5 font-mono text-[0.68rem] hover:text-accent">{id}</a>{/each}
              {#if (r.cited_by?.length ?? 0) > 5}<span class="text-faint">and {r.cited_by!.length - 5} more</span>{/if}
            </div>
          </div>
          <Button size="sm" busy={adding === r.title} onclick={() => add(r)}><Plus class="size-3.5" />Add</Button>
        </div>
      {:else}
        <div class="flex flex-col items-center gap-3 px-6 py-16 text-center">
          <Compass class="size-10 text-faint" />
          <div class="font-medium">Nothing to suggest yet</div>
          <p class="max-w-sm text-sm text-muted">Suggestions come from your papers' references; open a paper's References tab to look them up.</p>
        </div>
      {/each}
    </div>
  {/if}
</div>
