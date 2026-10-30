<script lang="ts">
  import { Check, FileText, Search, Split, UserRound, Users } from '@lucide/svelte';
  import { api, type Person } from '../lib/api';
  import Badge from '../lib/components/Badge.svelte';
  import Button from '../lib/components/Button.svelte';
  import Spinner from '../lib/components/Spinner.svelte';
  import { byline } from '../lib/markdown';
  import { href, paperHref, router } from '../lib/router.svelte';
  import { app } from '../lib/state.svelte';
  import { toasts } from '../lib/toast.svelte';

  // Who is who, like the TUI's People tab: everyone who wrote a library paper, with their spellings,
  // co-authors, and the people who might be them too.
  let people = $state<Person[] | null>(null);
  let filter = $state('');
  let busy = $state<string | null>(null);

  const load = async () => {
    try {
      people = await api.people();
    } catch (e) {
      toasts.error(e, 'Cannot load the people');
      people = [];
    }
  };
  load();

  const fold = (s: string) => s.normalize('NFD').replace(/[̀-ͯ]/g, '').toLowerCase();
  const shown = $derived((people ?? []).filter((p) => !filter || p.variants.some((v) => fold(v).includes(fold(filter)))));
  const selected = $derived(
    (people ?? []).find((p) => p.name === router.params.get('name')) ?? shown[0] ?? null,
  );

  async function decide(person: Person, other: Person['maybe'][number], same: boolean) {
    busy = other.name;
    try {
      const spellings = [...person.variants, ...other.spellings];
      if (same) await api.samePerson(spellings);
      // Different people: one spelling each sets them apart, as in the TUI.
      else await api.differentPeople([person.variants[0], other.spellings[0]]);
      toasts.show('success', same ? `${other.name} is ${person.name}` : `${other.name} is someone else`);
      await load();
    } catch (e) {
      toasts.error(e);
    }
    busy = null;
  }

  const personHref = (name: string) => href('/people', { name });
</script>

<div class="flex h-full">
  <div class="flex w-80 shrink-0 flex-col border-r">
    <div class="border-b p-3">
      <div class="relative">
        <Search class="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-faint" />
        <input bind:value={filter} placeholder="Find a person" class="h-9 w-full rounded-lg border bg-card pr-3 pl-9 text-sm outline-none placeholder:text-faint focus:border-accent" />
      </div>
      <p class="mt-2 px-1 text-xs text-faint">{shown.length} people, most papers first</p>
    </div>
    <div class="min-h-0 flex-1 overflow-y-auto p-2">
      {#if people === null}
        <div class="flex justify-center py-10"><Spinner class="size-5 text-faint" /></div>
      {/if}
      {#each shown as p (`${p.name}|${p.papers.join()}`)}
        <a
          href={personHref(p.name)}
          class="flex items-center gap-2 rounded-lg px-2.5 py-1.5 text-sm {selected === p ? 'bg-hover font-medium' : 'text-muted hover:bg-hover/60 hover:text-foreground'}"
        >
          <span class="min-w-0 flex-1 truncate">{p.name}</span>
          {#if p.id}<span title="Confirmed"><Check class="size-3.5 text-ready" /></span>{/if}
          {#if p.maybe.length}<span title="Someone might be the same person" class="text-xs font-semibold text-warning">?</span>{/if}
          <span class="text-xs text-faint tabular-nums">{p.papers.length}</span>
        </a>
      {/each}
    </div>
  </div>

  <div class="min-w-0 flex-1 overflow-y-auto">
    {#if selected}
      {@const p = selected}
      <div class="mx-auto max-w-4xl space-y-8 px-6 py-8 lg:px-10">
        <header class="flex items-start gap-4">
          <div class="flex size-12 shrink-0 items-center justify-center rounded-full bg-accent/15 text-accent"><UserRound class="size-6" /></div>
          <div>
            <h1 class="flex items-center gap-2 text-2xl font-semibold tracking-tight">
              {p.name}
              {#if p.id}<Badge tone="ready">confirmed</Badge>{/if}
            </h1>
            <p class="mt-1 text-sm text-muted">{p.papers.length} {p.papers.length === 1 ? 'paper' : 'papers'} in the library</p>
            {#if p.variants.length > 1}
              <div class="mt-2 flex flex-wrap gap-1.5">{#each p.variants as v}<Badge>{v}</Badge>{/each}</div>
            {/if}
          </div>
        </header>

        {#if p.maybe.length}
          <section class="overflow-hidden rounded-xl border border-warning/40">
            <div class="border-b border-warning/30 bg-warning/10 px-4 py-2.5 text-sm font-medium">Might be the same person</div>
            {#each p.maybe as m, i (i)}
              <div class="flex flex-wrap items-center gap-3 border-b px-4 py-3 last:border-b-0">
                <div class="min-w-0 flex-1">
                  <div class="text-sm font-medium">{m.name}</div>
                  <div class="text-xs text-muted">
                    {[m.papers.length ? `${m.papers.length} library papers` : 'cited only', m.with.length ? `with ${m.with.join(', ')}` : ''].filter(Boolean).join(' · ')}
                  </div>
                </div>
                <Button size="sm" variant="ghost" busy={busy === m.name} onclick={() => decide(p, m, false)}><Split class="size-3.5" />Different</Button>
                <Button size="sm" variant="primary" busy={busy === m.name} onclick={() => decide(p, m, true)}><Check class="size-3.5" />Same person</Button>
              </div>
            {/each}
          </section>
        {/if}

        <section>
          <h2 class="mb-2 text-xs font-semibold tracking-wide text-faint uppercase">Papers</h2>
          <div class="overflow-hidden rounded-xl border bg-card">
            {#each p.papers as id}
              {@const paper = app.papers[id]}
              <a href={paperHref(id)} class="flex items-start gap-3 border-b px-4 py-2.5 last:border-b-0 hover:bg-hover/50">
                <FileText class="mt-0.5 size-4 shrink-0 text-faint" />
                <div class="min-w-0">
                  <div class="text-sm font-medium">{paper?.title ?? id}</div>
                  {#if paper}<div class="text-xs text-muted">{byline(paper)}</div>{/if}
                </div>
              </a>
            {/each}
          </div>
        </section>

        {#if p.coauthors.length}
          <section>
            <h2 class="mb-2 flex items-center gap-2 text-xs font-semibold tracking-wide text-faint uppercase"><Users class="size-3.5" />Wrote with</h2>
            <div class="flex flex-wrap gap-1.5">
              {#each p.coauthors as [name, n]}
                {@const known = (people ?? []).some((x) => x.name === name)}
                <svelte:element
                  this={known ? 'a' : 'span'}
                  href={known ? personHref(name) : undefined}
                  class="inline-flex items-center gap-1.5 rounded-lg border bg-card px-2.5 py-1 text-sm {known ? 'hover:border-accent hover:text-accent' : 'text-muted'}"
                >
                  {name}<span class="text-xs text-faint tabular-nums">{n}</span>
                </svelte:element>
              {/each}
            </div>
            <p class="mt-2 text-xs text-faint">Counts include the papers your papers cite.</p>
          </section>
        {/if}
      </div>
    {:else if people !== null}
      <div class="flex h-full items-center justify-center text-sm text-muted">No one matches “{filter}”.</div>
    {/if}
  </div>
</div>
