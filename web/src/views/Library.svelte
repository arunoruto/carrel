<script lang="ts">
  import { Download, FileText, Plus, Search, Sparkles, X } from '@lucide/svelte';
  import { api, type PaperSummary } from '../lib/api';
  import AddDialog from '../lib/components/AddDialog.svelte';
  import Badge from '../lib/components/Badge.svelte';
  import Button from '../lib/components/Button.svelte';
  import Spinner from '../lib/components/Spinner.svelte';
  import Status from '../lib/components/Status.svelte';
  import { ago, authors } from '../lib/markdown';
  import { paperHref, router } from '../lib/router.svelte';
  import { app } from '../lib/state.svelte';

  const project = $derived(router.params.get('project') ?? undefined);
  let query = $state(router.params.get('q') ?? '');
  let semantic = $state(router.params.get('semantic') === '1');
  let sort = $state<'added' | 'year' | 'title' | 'relevance'>('added');
  let papers = $state<PaperSummary[]>([]);
  let loading = $state(true);
  let error = $state('');
  let adding = $state(false);
  let input: HTMLInputElement;

  // Searching as you type, a moment after the last key; meaning-search asks Ollama, so a bit later.
  $effect(() => {
    const q = query.trim();
    const [p, s, _] = [project, semantic, app.version];
    loading = true;
    const timer = setTimeout(async () => {
      try {
        const scoped = p ? `${q} project:${p}` : q;
        papers = q ? await api.search(scoped, s) : await api.papers(p);
        error = '';
      } catch (e) {
        error = e instanceof Error ? e.message : String(e);
        papers = [];
      }
      loading = false;
    }, q ? (s ? 400 : 150) : 0);
    return () => clearTimeout(timer);
  });

  const searching = $derived(query.trim().length > 0);
  // Searching sorts by match, and stops when the search is cleared.
  $effect(() => {
    if (searching && sort !== 'relevance') sort = 'relevance';
    if (!searching && sort === 'relevance') sort = 'added';
  });
  const shown = $derived.by(() => {
    const list = [...papers];
    if (searching && (sort === 'relevance' || semantic)) return list;
    if (sort === 'year') list.sort((a, b) => (b.year ?? 0) - (a.year ?? 0) || a.title.localeCompare(b.title));
    else if (sort === 'title') list.sort((a, b) => a.title.localeCompare(b.title));
    else if (sort === 'added') list.sort((a, b) => (b.added ?? 0) - (a.added ?? 0));
    return list;
  });

  // Semantic hits come as "0.66 full text at 11585: …passage…".
  function semanticHit(snippet: string) {
    const m = snippet.match(/^(\d\.\d\d) ([^:]+): ([\s\S]*)$/);
    if (!m) return null;
    const at = m[2].match(/at (\d+)/)?.[1];
    return { score: m[1], place: m[2], at, text: m[3].replace(/[*_#`]+|!?\[\]\([^)]*\)/g, '').replace(/\s+/g, ' ') };
  }

  // j/k move through the list as in the TUI, Enter opens.
  let cursor = $state(-1);
  $effect(() => {
    void shown;
    cursor = -1;
  });
  $effect(() => {
    if (cursor >= 0) document.getElementById(`row-${cursor}`)?.scrollIntoView({ block: 'nearest' });
  });

  function keys(e: KeyboardEvent) {
    const typing = (e.target as HTMLElement).closest('input, textarea, [contenteditable]');
    if (!typing && !e.metaKey && !e.ctrlKey) {
      if (e.key === 'j' || e.key === 'ArrowDown') return (e.preventDefault(), (cursor = Math.min(shown.length - 1, cursor + 1)));
      if (e.key === 'k' || e.key === 'ArrowUp') return (e.preventDefault(), (cursor = Math.max(0, cursor - 1)));
      if (e.key === 'Enter' && shown[cursor]) return (location.hash = paperHref(shown[cursor].id).slice(1));
    }
    if ((e.key === '/' && !typing) || (e.key === 'k' && (e.metaKey || e.ctrlKey))) {
      e.preventDefault();
      input.focus();
      input.select();
    }
  }
</script>

<svelte:window onkeydown={keys} />

<div class="mx-auto flex max-w-6xl flex-col gap-5 px-6 py-6 lg:px-10 lg:py-8">
  <header class="flex flex-wrap items-end justify-between gap-3">
    <div>
      <h1 class="text-2xl font-semibold tracking-tight">{project ?? 'Library'}</h1>
      <p class="mt-1 text-sm text-muted">
        {#if loading}Loading…{:else if searching && semantic}The {papers.length} closest in meaning{:else if searching}{papers.length} {papers.length === 1 ? 'match' : 'matches'}{:else}{papers.length} papers{/if}
        {#if project}· <a class="text-accent hover:underline" href="#/">all papers</a>{/if}
      </p>
    </div>
    <div class="flex gap-2">
      <a href={api.bibUrl(project)} download title="BibTeX of {project ?? 'every paper'}"><Button><Download class="size-4" />.bib</Button></a>
      <Button variant="primary" onclick={() => (adding = true)}><Plus class="size-4" />Add paper</Button>
    </div>
  </header>

  <div class="flex flex-wrap items-center gap-2">
    <div class="relative min-w-64 flex-1">
      <Search class="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-faint" />
      <input
        bind:this={input}
        bind:value={query}
        placeholder={semantic ? 'Describe what you are looking for…' : 'Search titles, authors, text…  author:hapke year:2012'}
        class="h-10 w-full rounded-lg border bg-card pr-20 pl-9 text-sm shadow-sm outline-none placeholder:text-faint focus:border-accent focus:ring-2 focus:ring-accent/20"
        onkeydown={(e) => e.key === 'Escape' && (query = '')}
      />
      <div class="absolute top-1/2 right-2 flex -translate-y-1/2 items-center gap-1">
        {#if loading && searching}<Spinner class="size-4 text-faint" />{/if}
        {#if query}<button class="cursor-pointer rounded p-1 text-faint hover:text-foreground" onclick={() => (query = '')} aria-label="Clear"><X class="size-3.5" /></button>{/if}
        <kbd class="rounded border bg-background px-1.5 text-[0.65rem] text-faint">/</kbd>
      </div>
    </div>
    <div class="flex h-10 rounded-lg border bg-card p-1 text-sm shadow-sm">
      <button class="cursor-pointer rounded-md px-3 transition {!semantic ? 'bg-hover font-medium' : 'text-muted'}" onclick={() => (semantic = false)}>Words</button>
      <button
        class="flex cursor-pointer items-center gap-1.5 rounded-md px-3 transition disabled:cursor-not-allowed disabled:opacity-40 {semantic ? 'bg-hover font-medium' : 'text-muted'}"
        onclick={() => (semantic = true)}
        disabled={!app.semantic}
        title={app.semantic ? `By meaning, with ${app.semantic.model}` : 'Semantic search needs [embeddings] in the config, and Ollama running'}
      >
        <Sparkles class="size-3.5" />Meaning
      </button>
    </div>
    <select bind:value={sort} class="h-10 cursor-pointer rounded-lg border bg-card px-3 text-sm shadow-sm outline-none" disabled={semantic && searching}>
      {#if searching}<option value="relevance">Best match</option>{/if}
      <option value="added">Recently added</option>
      <option value="year">Newest first</option>
      <option value="title">Title</option>
    </select>
  </div>

  {#if error}
    <div class="rounded-xl border border-failed/40 bg-failed/10 px-4 py-3 text-sm text-failed">{error}</div>
  {/if}

  <div class="overflow-hidden rounded-xl border bg-card shadow-sm">
    {#each shown as p, i (p.id)}
      {@const hitAt = p.snippet ? semanticHit(p.snippet)?.at : undefined}
      <a
        id="row-{i}"
        href={hitAt ? `${paperHref(p.id)}?at=${hitAt}` : paperHref(p.id)}
        class="group flex gap-4 border-b px-5 py-3.5 transition last:border-b-0 hover:bg-hover/50 {cursor === i ? 'bg-hover/70 shadow-[inset_3px_0_0_var(--accent)]' : ''}"
      >
        <div class="mt-0.5 flex size-9 shrink-0 items-center justify-center rounded-lg {p.pdf ? 'bg-accent/10 text-accent' : 'bg-hover text-faint'}">
          <FileText class="size-4.5" />
        </div>
        <div class="min-w-0 flex-1">
          <div class="flex items-start gap-3">
            <div class="min-w-0 flex-1 font-medium leading-snug group-hover:text-accent">{p.title || p.id}</div>
            <div class="flex shrink-0 gap-1"><Status pdf={p.pdf} text={p.text} drafts={p.drafts} quiet /></div>
          </div>
          <div class="mt-0.5 flex gap-1.5 truncate text-sm text-muted">
            <span>{authors(p.authors)}</span>
            {#if p.venue}<span class="text-faint">·</span><span class="truncate italic">{p.venue}</span>{/if}
            {#if p.year}<span class="text-faint">·</span><span>{p.year}</span>{/if}
          </div>
          {#if p.snippet}
            {@const hit = semanticHit(p.snippet)}
            {#if hit}
              <div class="mt-1.5 flex items-start gap-2 text-xs text-muted">
                <span class="shrink-0 rounded bg-accent/15 px-1.5 py-0.5 font-medium text-accent tabular-nums" title="Similarity">{hit.score}</span>
                <span class="line-clamp-2"><span class="text-faint">{hit.place}:</span> {hit.text}</span>
              </div>
            {:else}
              <div class="mt-1.5 line-clamp-2 text-xs text-muted [&_b]:font-semibold [&_b]:text-foreground">{@html p.snippet.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/\[(.+?)\]/g, '<b>$1</b>')}</div>
            {/if}
          {/if}
          {#if p.keywords.length}
            <div class="mt-2 flex flex-wrap gap-1">
              {#each p.keywords.slice(0, 6) as k}<Badge>{k}</Badge>{/each}
              {#if p.keywords.length > 6}<span class="text-[0.7rem] text-faint">+{p.keywords.length - 6}</span>{/if}
            </div>
          {/if}
        </div>
        {#if !searching && sort === 'added' && p.added}
          <div class="hidden shrink-0 text-xs text-faint sm:block">{ago(p.added)}</div>
        {/if}
      </a>
    {:else}
      {#if !loading}
        <div class="flex flex-col items-center gap-3 px-6 py-16 text-center">
          <FileText class="size-10 text-faint" />
          {#if searching}
            <div class="font-medium">Nothing matches “{query}”</div>
            <p class="max-w-sm text-sm text-muted">Try other words{#if app.semantic && !semantic}, or search by <button class="cursor-pointer text-accent hover:underline" onclick={() => (semantic = true)}>meaning</button>{/if}.</p>
          {:else}
            <div class="font-medium">No papers yet</div>
            <p class="max-w-sm text-sm text-muted">Add one by DOI, arXiv id or title, or drop PDFs anywhere on this page.</p>
            <Button variant="primary" onclick={() => (adding = true)}><Plus class="size-4" />Add paper</Button>
          {/if}
        </div>
      {:else}
        <div class="flex justify-center py-16"><Spinner class="size-6 text-faint" /></div>
      {/if}
    {/each}
  </div>
</div>

<AddDialog bind:open={adding} />
