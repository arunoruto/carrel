<script lang="ts">
  import { Compass, FilePen, Folder, HeartPulse, Inbox, Library, Moon, Network, RefreshCw, Sparkles, Sun, Users } from '@lucide/svelte';
  import { api } from '../api';
  import { href, router } from '../router.svelte';
  import { app } from '../state.svelte';
  import { toasts } from '../toast.svelte';
  import Logo from './Logo.svelte';
  import Spinner from './Spinner.svelte';

  let { onnavigate }: { onnavigate?: () => void } = $props();
  let dark = $state(document.documentElement.classList.contains('dark'));
  let syncing = $state(false);

  function theme() {
    dark = !dark;
    document.documentElement.classList.toggle('dark', dark);
    try {
      localStorage.setItem('theme', dark ? 'dark' : 'light');
    } catch {}
  }

  async function sync() {
    syncing = true;
    try {
      const { summary, failed } = await api.sync();
      toasts.show(failed.length ? 'error' : 'success', 'Synced', [summary, ...failed].join('\n'));
      app.changed();
    } catch (e) {
      toasts.error(e, 'Cannot sync');
    }
    syncing = false;
  }

  const project = $derived(router.path === '/' ? router.params.get('project') : null);
  const nav = $derived([
    { href: '#/', icon: Library, label: 'Library', count: app.total, active: router.path === '/' && !project },
    { href: '#/inbox', icon: Inbox, label: 'Inbox', count: app.inbox, active: router.path === '/inbox', loud: true },
    { href: '#/drafts', icon: FilePen, label: 'Drafts', count: app.info?.drafts ?? 0, active: router.path === '/drafts', loud: true },
    { href: '#/discover', icon: Compass, label: 'Discover', count: 0, active: router.path === '/discover' },
    { href: '#/map', icon: Network, label: 'Map', count: 0, active: router.path === '/map' },
    { href: '#/people', icon: Users, label: 'People', count: 0, active: router.path === '/people' },
    { href: '#/health', icon: HeartPulse, label: 'Health', count: 0, active: router.path === '/health' },
  ]);
</script>

<aside class="flex h-full w-60 shrink-0 flex-col border-r bg-sidebar">
  <a href="#/" class="flex items-center gap-2.5 px-5 pt-5 pb-4" onclick={onnavigate}>
    <Logo />
    <span class="text-lg font-semibold tracking-tight">carrel</span>
  </a>

  <nav class="space-y-0.5 px-3">
    {#each nav as item}
      <a
        href={item.href}
        onclick={onnavigate}
        class="flex items-center gap-2.5 rounded-lg px-2.5 py-1.5 text-sm transition {item.active ? 'bg-hover font-medium text-foreground' : 'text-muted hover:bg-hover/60 hover:text-foreground'}"
      >
        <item.icon class="size-4" />
        <span class="flex-1">{item.label}</span>
        {#if item.count}
          <span class="rounded-md px-1.5 text-xs {item.loud ? 'bg-accent/15 font-medium text-accent' : 'text-faint'}">{item.count}</span>
        {/if}
      </a>
    {/each}
  </nav>

  <div class="mt-6 px-5 pb-1.5 text-xs font-medium tracking-wide text-faint uppercase">Projects</div>
  <div class="min-h-0 flex-1 space-y-0.5 overflow-y-auto px-3">
    {#each app.projects as p}
      <a
        href={href('/', { project: p.name })}
        onclick={onnavigate}
        class="flex items-center gap-2.5 rounded-lg px-2.5 py-1.5 text-sm transition {project === p.name ? 'bg-hover font-medium text-foreground' : 'text-muted hover:bg-hover/60 hover:text-foreground'}"
      >
        <Folder class="size-4" />
        <span class="flex-1 truncate">{p.name}</span>
        <span class="text-xs text-faint">{p.papers.length}</span>
      </a>
    {:else}
      <p class="px-2.5 text-xs text-faint">None yet. Put a paper in one from its page.</p>
    {/each}
  </div>

  <div class="space-y-2 border-t px-3 py-3">
    {#if app.info?.embeddings}
      {@const e = app.info.embeddings}
      <div class="flex items-center gap-2 px-2.5 text-xs text-muted" title={'error' in e ? e.error : `${e.model} on ${e.url}`}>
        <Sparkles class="size-3.5 {'error' in e ? 'text-failed' : 'text-ready'}" />
        <span class="truncate">{'error' in e ? 'Semantic search is down' : `Semantic search: ${e.model}`}</span>
      </div>
    {/if}
    <div class="flex items-center gap-1">
      {#if app.info?.remote}
        <button class="flex flex-1 cursor-pointer items-center gap-2 rounded-lg px-2.5 py-1.5 text-sm text-muted hover:bg-hover hover:text-foreground" onclick={sync} disabled={syncing}>
          {#if syncing}<Spinner />{:else}<RefreshCw class="size-4" />{/if}
          {syncing ? 'Syncing…' : 'Sync'}
        </button>
      {:else}
        <div class="flex-1 px-2.5 text-xs text-faint">v{app.info?.version ?? ''}</div>
      {/if}
      <button class="cursor-pointer rounded-lg p-2 text-muted hover:bg-hover hover:text-foreground" onclick={theme} aria-label="Switch theme" title="Switch theme">
        {#if dark}<Sun class="size-4" />{:else}<Moon class="size-4" />{/if}
      </button>
    </div>
  </div>
</aside>
