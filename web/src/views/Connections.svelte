<script lang="ts">
  import { CornerDownRight, Network, Repeat, Sparkles, Users } from '@lucide/svelte';
  import { api, type Neighbors, type TreeNode } from '../lib/api';
  import Spinner from '../lib/components/Spinner.svelte';
  import { byline } from '../lib/markdown';
  import { paperHref } from '../lib/router.svelte';
  import { app } from '../lib/state.svelte';

  // How a paper connects to the rest of the library: the citation tree among library papers (as the
  // TUI's Graph tab and `carrel tree` show it), the papers most alike by their embeddings, and the
  // papers sharing its authors.
  let { id }: { id: string } = $props();
  let citedBy = $state(false);
  let depth = $state(2);
  let tree = $state<TreeNode[] | null>(null);
  let similar = $state<{ id: string; score: number }[] | null>(null);
  let neighbors = $state<Neighbors | null>(null);

  $effect(() => {
    const [paper, up, d] = [id, citedBy, depth];
    tree = null;
    api.tree(paper, up, d).then((t) => (tree = t)).catch(() => (tree = []));
  });
  $effect(() => {
    void id;
    api.similar(id).then((s) => (similar = s)).catch(() => (similar = []));
    api.neighbors(id).then((n) => (neighbors = n)).catch(() => (neighbors = null));
  });

  const title = (other: string) => app.papers[other]?.title ?? other;
  const shared = $derived(Object.entries(neighbors?.shared_authors ?? {}).sort((a, b) => b[1].length - a[1].length));
</script>

<div class="space-y-10">
  <a href="#/map?focus={encodeURIComponent(id)}" class="inline-flex items-center gap-2 rounded-lg border bg-card px-3 py-1.5 text-sm hover:border-accent hover:text-accent">
    <Network class="size-4" />Show on the map
  </a>
  <section>
    <div class="mb-3 flex flex-wrap items-center gap-3">
      <h2 class="text-xs font-semibold tracking-wide text-faint uppercase">Citation tree</h2>
      <div class="flex rounded-lg border bg-card p-0.5 text-xs">
        <button class="cursor-pointer rounded-md px-2.5 py-1 {!citedBy ? 'bg-hover font-medium' : 'text-muted'}" onclick={() => (citedBy = false)}>What it cites</button>
        <button class="cursor-pointer rounded-md px-2.5 py-1 {citedBy ? 'bg-hover font-medium' : 'text-muted'}" onclick={() => (citedBy = true)}>What cites it</button>
      </div>
      <label class="flex items-center gap-2 text-xs text-muted">
        levels
        <select bind:value={depth} class="h-7 cursor-pointer rounded-md border bg-card px-1.5 text-xs outline-none">
          {#each [1, 2, 3, 4, 5] as d}<option value={d}>{d}</option>{/each}
        </select>
      </label>
    </div>
    {#if tree === null}
      <div class="py-4"><Spinner class="size-5 text-faint" /></div>
    {:else if tree.length === 0}
      <p class="text-sm text-faint">
        {citedBy ? 'No library paper cites it.' : neighbors && !neighbors.references_known ? 'Its references aren’t looked up yet; the References tab does that.' : 'It cites no library paper.'}
      </p>
    {:else}
      <ul class="text-sm">
        {#each tree as node, i (i)}
          <li class="flex items-start gap-1.5 py-0.5" style="padding-left: {(node.level - 1) * 1.5}rem">
            <CornerDownRight class="mt-1 size-3.5 shrink-0 text-faint" />
            {#if node.again}
              <a href={paperHref(node.id)} class="flex min-w-0 items-center gap-1.5 text-faint hover:text-accent" title="Listed above already">
                <span class="truncate">{title(node.id)}</span><Repeat class="size-3 shrink-0" />
              </a>
            {:else}
              <a href={paperHref(node.id)} class="min-w-0 hover:text-accent">
                <span class={node.level === 1 ? 'font-medium' : ''}>{title(node.id)}</span>
                {#if app.papers[node.id]}<span class="text-xs text-faint"> · {byline(app.papers[node.id])}</span>{/if}
              </a>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  </section>

  <section>
    <h2 class="mb-3 flex items-center gap-2 text-xs font-semibold tracking-wide text-faint uppercase"><Sparkles class="size-3.5" />Most alike</h2>
    {#if similar === null}
      <div class="py-4"><Spinner class="size-5 text-faint" /></div>
    {:else if similar.length === 0}
      <p class="text-sm text-faint">Needs embeddings; see Semantic search in the docs.</p>
    {:else}
      <ul class="space-y-1">
        {#each similar as s}
          <li>
            <a href={paperHref(s.id)} class="flex items-start gap-3 rounded-lg px-2 py-1.5 hover:bg-hover/50">
              <span class="mt-0.5 shrink-0 rounded bg-accent/15 px-1.5 py-0.5 text-xs font-medium text-accent tabular-nums">{s.score.toFixed(2)}</span>
              <span class="min-w-0 text-sm">
                {title(s.id)}
                {#if app.papers[s.id]}<span class="block text-xs text-muted">{byline(app.papers[s.id])}</span>{/if}
              </span>
            </a>
          </li>
        {/each}
      </ul>
      <p class="mt-2 text-xs text-faint">By title, keywords and abstract, from the stored embeddings.</p>
    {/if}
  </section>

  {#if shared.length}
    <section>
      <h2 class="mb-3 flex items-center gap-2 text-xs font-semibold tracking-wide text-faint uppercase"><Users class="size-3.5" />Same authors</h2>
      <ul class="space-y-1">
        {#each shared as [other, names]}
          <li>
            <a href={paperHref(other)} class="block rounded-lg px-2 py-1.5 hover:bg-hover/50">
              <span class="text-sm">{title(other)}</span>
              <span class="block text-xs text-muted">with {names.join(', ')}</span>
            </a>
          </li>
        {/each}
      </ul>
    </section>
  {/if}
</div>
