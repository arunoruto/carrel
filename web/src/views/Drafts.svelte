<script lang="ts">
  import { ArrowUpRight, Check, FilePen, X } from '@lucide/svelte';
  import { api, type Draft } from '../lib/api';
  import Badge from '../lib/components/Badge.svelte';
  import Button from '../lib/components/Button.svelte';
  import Spinner from '../lib/components/Spinner.svelte';
  import { markdown } from '../lib/markdown';
  import { paperHref } from '../lib/router.svelte';
  import { app } from '../lib/state.svelte';
  import { toasts } from '../lib/toast.svelte';

  // What AIs proposed, next to what it would change; nothing of theirs counts until approved here.
  let drafts = $state<Draft[] | null>(null);
  let busy = $state<string | null>(null);

  const load = async () => (drafts = await api.drafts().catch((e) => (toasts.error(e), [])));
  load();

  async function decide(d: Draft, approve: boolean) {
    busy = `${d.id}/${d.field}`;
    try {
      await api.decide(d.id, d.field, approve);
      toasts.show('success', `${approve ? 'Approved' : 'Rejected'} the ${d.field} of ${d.id}`);
      await load();
      app.changed();
    } catch (e) {
      toasts.error(e);
    }
    busy = null;
  }

  const show = (d: Draft, text: string | null) => {
    if (!text) return '<p class="text-faint">Nothing yet.</p>';
    if (d.field === 'keywords') return text.split(/\n|, /).filter(Boolean).map((k) => `<code>${k.replace(/</g, '&lt;')}</code>`).join(' ');
    if (d.field === 'references') return `<pre>${text.slice(0, 4000).replace(/</g, '&lt;')}</pre>`;
    return markdown(text);
  };
</script>

<div class="mx-auto flex max-w-6xl flex-col gap-5 px-6 py-6 lg:px-10 lg:py-8">
  <header>
    <h1 class="text-2xl font-semibold tracking-tight">Drafts</h1>
    <p class="mt-1 text-sm text-muted">What an AI proposed. Approving keeps it; rejecting throws it away.</p>
  </header>

  {#if drafts === null}
    <div class="flex justify-center py-16"><Spinner class="size-6 text-faint" /></div>
  {:else}
    {#each drafts as d (`${d.id}/${d.field}`)}
      <article class="overflow-hidden rounded-xl border bg-card shadow-sm">
        <header class="flex flex-wrap items-center gap-3 border-b px-5 py-3">
          <Badge tone="warning">{d.field}</Badge>
          <a href={paperHref(d.id)} class="flex min-w-0 flex-1 items-center gap-1 font-medium hover:text-accent">
            <span class="truncate">{d.title}</span><ArrowUpRight class="size-4 shrink-0 text-faint" />
          </a>
          <Button size="sm" variant="ghost" busy={busy === `${d.id}/${d.field}`} onclick={() => decide(d, false)}><X class="size-3.5" />Reject</Button>
          <Button size="sm" variant="primary" busy={busy === `${d.id}/${d.field}`} onclick={() => decide(d, true)}><Check class="size-3.5" />Approve</Button>
        </header>
        <div class="grid divide-y md:grid-cols-2 md:divide-x md:divide-y-0">
          <div class="max-h-96 overflow-y-auto px-5 py-4">
            <div class="mb-2 text-xs font-semibold tracking-wide text-faint uppercase">Now</div>
            <div class="prose text-muted">{@html show(d, d.current)}</div>
          </div>
          <div class="max-h-96 overflow-y-auto bg-warning/5 px-5 py-4">
            <div class="mb-2 text-xs font-semibold tracking-wide text-warning uppercase">Proposed{d.field === 'summary' ? '' : ', added to what is there'}</div>
            <div class="prose">{@html show(d, d.draft)}</div>
          </div>
        </div>
      </article>
    {:else}
      <div class="flex flex-col items-center gap-3 rounded-xl border bg-card px-6 py-16 text-center">
        <FilePen class="size-10 text-faint" />
        <div class="font-medium">No drafts waiting</div>
        <p class="max-w-sm text-sm text-muted">When an AI proposes a summary, keywords, notes or references over MCP, they wait here for you.</p>
      </div>
    {/each}
  {/if}
</div>
