<script lang="ts">
  import { Compass, ExternalLink, EyeOff, Mail, Plus } from '@lucide/svelte';
  import { addSource } from '../lib/actions';
  import { api, type Recommendation, type Reference } from '../lib/api';
  import Badge from '../lib/components/Badge.svelte';
  import Button from '../lib/components/Button.svelte';
  import Spinner from '../lib/components/Spinner.svelte';
  import { byline } from '../lib/markdown';
  import { paperHref } from '../lib/router.svelte';
  import { app } from '../lib/state.svelte';
  import { toasts } from '../lib/toast.svelte';

  // Where the library grows: papers recommended by alert emails, and the ones its papers cite most but it lacks.
  let tab = $state<'recommended' | 'cited' | null>(null);
  let project = $state('');
  let list = $state<Reference[] | null>(null);
  let recommended = $state<Recommendation[] | null>(null);
  let error = $state('');
  let adding = $state<string | null>(null);
  let busy = $state<Set<string>>(new Set());

  api
    .recommended()
    .then((r) => ((recommended = r), (tab ??= r.length ? 'recommended' : 'cited')))
    .catch((e) => ((error = e.message), (recommended = []), (tab ??= 'cited')));

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

  async function act(r: Recommendation, add: boolean) {
    busy = new Set(busy).add(r.key);
    try {
      if (add) {
        const { id, messages } = await api.addRecommended(r.key);
        toasts.show('success', `Added ${id}`, [r.title, ...messages].join('\n'));
      } else {
        await api.dismiss(r.key);
      }
      recommended = recommended?.filter((x) => x.key !== r.key) ?? null;
      app.changed();
    } catch (e) {
      toasts.error(e, `Cannot add ${r.title}`);
    }
    const next = new Set(busy);
    next.delete(r.key);
    busy = next;
  }

  const authors = (r: Recommendation) => r.authors.join(', ');
  const tabClass = (on: boolean) =>
    `cursor-pointer rounded-md px-3 py-1 text-sm font-medium transition ${on ? 'bg-card shadow-sm' : 'text-muted hover:text-foreground'}`;
</script>

<div class="mx-auto flex max-w-4xl flex-col gap-5 px-6 py-6 lg:px-10 lg:py-8">
  <header class="flex flex-wrap items-end justify-between gap-3">
    <div>
      <h1 class="text-2xl font-semibold tracking-tight">Discover</h1>
      <p class="mt-1 text-sm text-muted">
        {tab === 'cited' ? 'What your papers cite most, but your library lacks.' : 'Papers your alert emails recommend, closest to your library first.'}
      </p>
    </div>
    <div class="flex flex-wrap items-center gap-2">
      <div class="flex rounded-lg bg-hover p-0.5" role="tablist">
        <button role="tab" aria-selected={tab === 'recommended'} class={tabClass(tab === 'recommended')} onclick={() => (tab = 'recommended')}>
          Recommended{#if recommended?.length}<span class="ml-1.5 text-xs text-faint">{recommended.length}</span>{/if}
        </button>
        <button role="tab" aria-selected={tab === 'cited'} class={tabClass(tab === 'cited')} onclick={() => (tab = 'cited')}>Cited but missing</button>
      </div>
      {#if tab === 'cited'}
        <select bind:value={project} class="h-9 cursor-pointer rounded-lg border bg-card px-3 text-sm shadow-sm outline-none">
          <option value="">Whole library</option>
          {#each app.projects as p}<option value={p.name}>{p.name}</option>{/each}
        </select>
      {/if}
    </div>
  </header>

  {#if error}<div class="rounded-xl border border-failed/40 bg-failed/10 px-4 py-3 text-sm text-failed">{error}</div>{/if}

  {#if tab === 'recommended'}
    {#if recommended === null}
      <div class="flex justify-center py-16"><Spinner class="size-6 text-faint" /></div>
    {:else}
      <div class="overflow-hidden rounded-xl border bg-card shadow-sm">
        {#each recommended as r (r.key)}
          <div class="flex items-start gap-4 border-b px-5 py-3.5 last:border-b-0">
            <div class="min-w-0 flex-1">
              <a href={r.url} target="_blank" rel="noopener noreferrer" class="group text-sm font-medium hover:text-accent">
                {r.title}<ExternalLink class="ml-1 inline size-3 text-faint group-hover:text-accent" />
              </a>
              <div class="mt-0.5 text-xs text-muted">
                {[authors(r), r.venue, r.year].filter(Boolean).join(' · ')}
              </div>
              {#if r.snippet}<p class="mt-1 line-clamp-2 text-xs text-faint">{r.snippet}</p>{/if}
              <div class="mt-1.5 flex flex-wrap items-center gap-1">
                {#if r.score != null}<Badge tone="accent" title="How alike to your library's papers">{Math.round(r.score * 100)}% alike</Badge>{/if}
                {#if r.pdf}<Badge tone="ready">PDF</Badge>{/if}
                {#if r.dates.length > 1}<Badge tone="wanted">recommended {r.dates.length}×</Badge>{/if}
                {#if r.dates.length}<span class="text-[0.7rem] text-faint">{r.dates[r.dates.length - 1]}</span>{/if}
              </div>
            </div>
            {#if busy.has(r.key)}
              <div class="flex items-center gap-2 pt-1 text-xs text-muted"><Spinner />Working…</div>
            {:else}
              <Button size="sm" variant="ghost" onclick={() => act(r, false)} title="Not interested; don't show it again"><EyeOff class="size-3.5" />Dismiss</Button>
              <Button size="sm" onclick={() => act(r, true)}><Plus class="size-3.5" />Add</Button>
            {/if}
          </div>
        {:else}
          <div class="flex flex-col items-center gap-3 px-6 py-16 text-center">
            <Mail class="size-10 text-faint" />
            <div class="font-medium">No recommendations yet</div>
            <p class="max-w-md text-sm text-muted">
              Save Google Scholar's alert emails (<code>.eml</code>) into a folder <code>carrel consume</code> watches, or run
              <code>carrel recommended import &lt;folder&gt;</code>.
              <a href="https://arunoruto.github.io/carrel/docs/guide/recommendations/" target="_blank" rel="noopener noreferrer" class="text-accent hover:underline">How to set it up</a>
            </p>
          </div>
        {/each}
      </div>
    {/if}
  {:else if tab === 'cited'}
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
  {:else}
    <div class="flex justify-center py-16"><Spinner class="size-6 text-faint" /></div>
  {/if}
</div>
