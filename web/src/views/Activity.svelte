<script lang="ts">
  import { ChevronDown, ChevronRight, Check, History, RotateCcw, Trash2, X } from '@lucide/svelte';
  import { api, type Activity } from '../lib/api';
  import Badge from '../lib/components/Badge.svelte';
  import Button from '../lib/components/Button.svelte';
  import { diff, type Line } from '../lib/diff';
  import { ago } from '../lib/markdown';
  import { paperHref } from '../lib/router.svelte';
  import { app } from '../lib/state.svelte';
  import { toasts } from '../lib/toast.svelte';

  // A server's changes, revertible by admins for 30 days, and the deletions people asked for.
  let entries = $state<Activity[] | null>(null);
  let tab = $state<'changes' | 'deletions'>('changes');
  let busy = $state<string | null>(null);
  /** Open diffs by `<entry id> <path>`. */
  let diffs = $state<Record<string, Line[] | 'loading'>>({});

  const load = () =>
    api
      .activity()
      .then((a) => (entries = a))
      .catch((e) => ((entries = []), toasts.error(e, 'Cannot load the activity')));
  load();

  const changes = $derived((entries ?? []).filter((e) => !e.status));
  const order = { pending: 0, accepted: 1, declined: 2, restored: 3 };
  const deletions = $derived((entries ?? []).filter((e) => e.status).sort((a, b) => order[a.status!] - order[b.status!]));
  const textual = (path: string) => /\.(bib|md|txt|json|tex|csv|toml)$/i.test(path);
  const title = (id: string | null) => (id && app.papers[id]?.title) || id;

  async function act(e: Activity, what: () => Promise<unknown>, done: string) {
    busy = e.id;
    try {
      await what();
      toasts.show('success', done);
      await load();
      app.changed();
    } catch (err) {
      toasts.error(err);
    }
    busy = null;
  }

  async function toggle(e: Activity, path: string) {
    const key = `${e.id} ${path}`;
    if (diffs[key]) return delete diffs[key];
    diffs[key] = 'loading';
    try {
      const { before, after } = await api.activityDiff(e.id, path);
      diffs[key] = diff(before ?? '', after ?? '');
    } catch (err) {
      delete diffs[key];
      toasts.error(err, 'Cannot show the change');
    }
  }

  const changeTone = { added: 'ready', changed: 'accent', removed: 'failed', delete: 'failed' } as const;
  const statusTone = { pending: 'wanted', accepted: 'failed', declined: 'neutral', restored: 'ready' } as const;
  const tabClass = (on: boolean) =>
    `cursor-pointer rounded-md px-3 py-1 text-sm font-medium transition ${on ? 'bg-card shadow-sm' : 'text-muted hover:text-foreground'}`;
</script>

{#snippet head(e: Activity)}
  <div class="flex flex-wrap items-baseline gap-x-1.5 text-sm">
    <span class="font-medium">{e.who}</span>
    {#if e.client}<span class="text-faint" title="What the client said it was; only a token proves who it is">· {e.client}</span>{/if}
    <span class="text-muted">{e.what}</span>
    {#if e.paper}<a href={paperHref(e.paper)} class="min-w-0 truncate hover:underline">{title(e.paper)}</a>{/if}
    <span class="text-xs text-faint">· {ago(e.at)}</span>
  </div>
{/snippet}

<div class="mx-auto flex max-w-4xl flex-col gap-5 px-6 py-6 lg:px-10 lg:py-8">
  <header class="flex flex-wrap items-end justify-between gap-3">
    <div>
      <h1 class="text-2xl font-semibold tracking-tight">Activity</h1>
      <p class="mt-1 text-sm text-muted">
        {tab === 'changes'
          ? 'Everything changed on this server in the last 30 days. Admins can revert any of it.'
          : 'Deletions people asked for. An admin other than the one asking decides; deleted files stay restorable for 30 days.'}
      </p>
    </div>
    <div class="flex rounded-lg bg-hover p-0.5" role="tablist">
      <button role="tab" aria-selected={tab === 'changes'} class={tabClass(tab === 'changes')} onclick={() => (tab = 'changes')}>
        Changes {changes.length || ''}
      </button>
      <button role="tab" aria-selected={tab === 'deletions'} class={tabClass(tab === 'deletions')} onclick={() => (tab = 'deletions')}>
        Deletions {app.pending || ''}
      </button>
    </div>
  </header>

  {#if entries === null}
    <p class="text-sm text-muted">Loading…</p>
  {:else if tab === 'changes'}
    <div class="divide-y rounded-xl border bg-card">
      {#each changes as e (e.id)}
        <div class="space-y-2 px-4 py-3">
          <div class="flex items-start gap-3">
            <div class="min-w-0 flex-1">{@render head(e)}</div>
            {#if e.decided}
              <span class="text-xs text-faint">reverted by {e.decided[0]}, {ago(e.decided[1])}</span>
            {:else if app.can.admin}
              <Button size="sm" busy={busy === e.id} onclick={() => act(e, () => api.revert(e.id), 'Reverted')}><RotateCcw class="size-3.5" />Revert</Button>
            {/if}
          </div>
          <ul class="space-y-1">
            {#each e.files as f}
              {@const key = `${e.id} ${f.path}`}
              {@const open = diffs[key]}
              <li>
                <div class="flex items-center gap-2 text-xs">
                  <Badge tone={changeTone[f.change]}>{f.change}</Badge>
                  {#if textual(f.path) && f.change !== 'delete'}
                    <button class="flex min-w-0 cursor-pointer items-center gap-1 font-mono text-muted hover:text-foreground" onclick={() => toggle(e, f.path)}>
                      {#if open}<ChevronDown class="size-3.5 shrink-0" />{:else}<ChevronRight class="size-3.5 shrink-0" />{/if}
                      <span class="truncate">{f.path}</span>
                    </button>
                  {:else}
                    <span class="truncate font-mono text-muted">{f.path}</span>
                  {/if}
                </div>
                {#if open === 'loading'}
                  <p class="mt-1 ml-6 text-xs text-faint">Loading…</p>
                {:else if open}
                  <pre class="mt-1 ml-6 overflow-x-auto rounded-lg bg-hover py-1.5 font-mono text-xs leading-relaxed">{#each open as line}{#if line.kind === '…'}<div class="px-3 text-faint">… {line.skipped} unchanged</div>{:else}<div class="px-3 {line.kind === '+' ? 'bg-ready/15 text-ready' : line.kind === '-' ? 'bg-failed/15 text-failed' : 'text-muted'}">{line.kind} {line.text}</div>{/if}{/each}</pre>
                {/if}
              </li>
            {/each}
          </ul>
        </div>
      {:else}
        <p class="flex items-center gap-2 px-4 py-6 text-sm text-muted"><History class="size-4" />No changes in the last 30 days.</p>
      {/each}
    </div>
  {:else}
    <div class="divide-y rounded-xl border bg-card">
      {#each deletions as e (e.id)}
        {@const own = e.who === app.me?.user}
        <div class="flex flex-wrap items-start gap-3 px-4 py-3">
          <div class="min-w-0 flex-1 space-y-1">
            {@render head(e)}
            <div class="flex flex-wrap items-center gap-2 text-xs text-muted">
              <Badge tone={statusTone[e.status!]}>{e.status}</Badge>
              <span title={e.files.map((f) => f.path).join('\n')}>{e.files.length} {e.files.length === 1 ? 'file' : 'files'}</span>
              {#if e.decided}<span class="text-faint">· {e.status === 'restored' ? 'restored' : 'decided'} by {e.decided[0]}, {ago(e.decided[1])}</span>{/if}
            </div>
          </div>
          {#if app.can.admin && e.status === 'pending'}
            <div class="flex gap-2" title={own ? 'Someone else decides' : undefined}>
              <Button size="sm" busy={busy === e.id} disabled={own} onclick={() => act(e, () => api.decideDeletion(e.id, false), 'Declined')}><X class="size-3.5" />Decline</Button>
              <Button size="sm" variant="danger" busy={busy === e.id} disabled={own} onclick={() => act(e, () => api.decideDeletion(e.id, true), 'Deleted; restorable for 30 days')}><Trash2 class="size-3.5" />Accept</Button>
            </div>
          {:else if app.can.admin && e.status === 'accepted'}
            <Button size="sm" busy={busy === e.id} onclick={() => act(e, () => api.revert(e.id), 'Restored')}><RotateCcw class="size-3.5" />Restore</Button>
          {/if}
        </div>
      {:else}
        <p class="flex items-center gap-2 px-4 py-6 text-sm text-muted"><Check class="size-4" />Nobody asked to delete anything.</p>
      {/each}
    </div>
  {/if}
</div>
