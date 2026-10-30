<script lang="ts">
  import { CircleAlert, CircleCheck, Copy, Info, Sparkles, TriangleAlert } from '@lucide/svelte';
  import { api, type Issue, type LibraryMap } from '../lib/api';
  import Spinner from '../lib/components/Spinner.svelte';
  import { byline } from '../lib/markdown';
  import { paperHref } from '../lib/router.svelte';
  import { app } from '../lib/state.svelte';

  // `carrel doctor` in the browser, plus papers so alike by their embeddings that they may be one paper twice.
  let issues = $state<Issue[] | null>(null);
  let twins = $state<[string, string, number][] | null>(null);
  let level = $state<'all' | Issue['level']>('all');
  api.doctor().then((i) => (issues = i)).catch(() => (issues = []));
  api.map().then((m: LibraryMap) => (twins = m.similar.filter(([, , s]) => s >= 0.93).sort((a, b) => b[2] - a[2]))).catch(() => (twins = []));

  const icons = { error: CircleAlert, warning: TriangleAlert, info: Info };
  const tones = { error: 'text-failed', warning: 'text-warning', info: 'text-accent' };
  const count = (l: Issue['level']) => issues?.filter((i) => i.level === l).length ?? 0;
  const shown = $derived((issues ?? []).filter((i) => level === 'all' || i.level === level));
  const title = (id: string) => app.papers[id]?.title ?? id;
</script>

<div class="mx-auto flex max-w-5xl flex-col gap-6 px-6 py-6 lg:px-10 lg:py-8">
  <header>
    <h1 class="text-2xl font-semibold tracking-tight">Health</h1>
    <p class="mt-1 text-sm text-muted">What <code class="rounded bg-hover px-1">carrel doctor</code> finds, each with the command that fixes it.</p>
  </header>

  {#if twins?.length}
    <section class="overflow-hidden rounded-xl border border-warning/40">
      <div class="flex items-center gap-2 border-b border-warning/30 bg-warning/10 px-4 py-2.5 text-sm font-medium">
        <Sparkles class="size-4 text-warning" />Possibly the same paper twice
      </div>
      {#each twins as [a, b, score]}
        <div class="flex items-center gap-4 border-b px-4 py-3 last:border-b-0">
          <span class="shrink-0 rounded bg-warning/15 px-1.5 py-0.5 text-xs font-medium text-warning tabular-nums">{score.toFixed(2)}</span>
          <div class="grid min-w-0 flex-1 gap-1 text-sm">
            {#each [a, b] as id}
              <a href={paperHref(id)} class="truncate hover:text-accent"><span class="font-mono text-xs text-faint">{id}</span> {title(id)}{#if app.papers[id]}<span class="text-xs text-muted"> · {byline(app.papers[id])}</span>{/if}</a>
            {/each}
          </div>
        </div>
      {/each}
      <p class="border-t px-4 py-2 text-xs text-faint">Alike by their embeddings. If one is a copy, merge what you need and delete it.</p>
    </section>
  {/if}

  {#if issues === null}
    <div class="flex justify-center py-10"><Spinner class="size-6 text-faint" /></div>
  {:else if issues.length === 0}
    <div class="flex flex-col items-center gap-3 rounded-xl border bg-card py-14 text-center">
      <CircleCheck class="size-10 text-ready" />
      <div class="font-medium">All good</div>
    </div>
  {:else}
    <div class="flex rounded-lg border bg-card p-0.5 text-xs self-start">
      {#each [['all', `All ${issues.length}`], ['error', `Errors ${count('error')}`], ['warning', `Warnings ${count('warning')}`], ['info', `Notes ${count('info')}`]] as [key, label]}
        <button class="cursor-pointer rounded-md px-2.5 py-1 {level === key ? 'bg-hover font-medium' : 'text-muted'}" onclick={() => (level = key as typeof level)}>{label}</button>
      {/each}
    </div>
    <div class="overflow-hidden rounded-xl border bg-card">
      {#each shown as issue}
        {@const Icon = icons[issue.level]}
        <div class="flex items-start gap-3 border-b px-4 py-3 last:border-b-0">
          <Icon class="mt-0.5 size-4 shrink-0 {tones[issue.level]}" />
          <div class="min-w-0 flex-1 text-sm">
            <div>
              {#if app.papers[issue.id]}<a href={paperHref(issue.id)} class="font-mono text-xs text-accent hover:underline">{issue.id}</a>{:else}<span class="font-mono text-xs text-muted">{issue.id}</span>{/if}
              <span class="ml-1 text-xs text-faint">{issue.check}</span>
            </div>
            <div class="mt-0.5">{issue.message}</div>
          </div>
          {#if issue.fix}
            <button class="flex shrink-0 cursor-pointer items-center gap-1.5 rounded-md bg-hover px-2 py-1 font-mono text-xs text-muted hover:text-foreground" title="Copy" onclick={() => navigator.clipboard.writeText(issue.fix!)}>
              {issue.fix}<Copy class="size-3" />
            </button>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</div>
