<script lang="ts">
  import Badge from './Badge.svelte';
  import Button from './Button.svelte';
  import Dialog from './Dialog.svelte';
  import { CircleUser, Compass, FilePen, Folder, FolderLock, FolderPlus, HeartPulse, History, Inbox, KeyRound, Library, Lock, LockOpen, LogIn, LogOut, Moon, Network, Pencil, RefreshCw, Sparkles, Sun, Trash2, Users } from '@lucide/svelte';
  import type { Project } from '../api';
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

  // Projects: a new one, renaming one, and the one waiting for its deletion to be confirmed.
  let creating = $state(false);
  let newName = $state('');
  let newPrivate = $state(false);
  let renaming = $state<string | null>(null);
  let renamed = $state('');
  let deleting = $state<Project | null>(null);
  let confirming = $state(false);

  async function projectCall(work: () => Promise<unknown>, done?: string) {
    try {
      await work();
      if (done) toasts.show('success', done);
      app.changed();
      return true;
    } catch (e) {
      toasts.error(e);
      return false;
    }
  }

  async function create(e: SubmitEvent) {
    e.preventDefault();
    const name = newName.trim();
    if (!name) return;
    if (await projectCall(() => api.createProject(name, newPrivate))) {
      newName = '';
      newPrivate = false;
      creating = false;
    }
  }

  async function rename(e: SubmitEvent, p: Project) {
    e.preventDefault();
    const name = renamed.trim();
    if (name && name !== p.name && !(await projectCall(() => api.changeProject(p.name, { name })))) return;
    renaming = null;
  }

  async function removeProject() {
    const p = deleting;
    confirming = false;
    if (!p) return;
    try {
      const { asked } = await api.deleteProject(p.name);
      toasts.show('success', asked ? 'Asked to delete; an admin decides' : `Deleted ${p.name}`);
      app.changed();
    } catch (e) {
      toasts.error(e);
    }
  }

  // A private project is its owner's alone; public ones are changed by members, and on a server only asked to delete.
  const mayChange = (p: Project) => (p.private ? !!app.me?.user : app.can.edit);
  const asks = (p: Project) => !p.private && !!app.me?.server;
  const mayDelete = (p: Project) => (p.private ? !!app.me?.user : asks(p) ? app.can.edit : app.can.admin);

  async function logout() {
    await api.logout();
    location.reload();
  }

  const me = $derived(app.me);
  const project = $derived(router.path === '/' ? router.params.get('project') : null);
  const nav = $derived([
    { href: '#/', icon: Library, label: 'Library', count: app.total, active: router.path === '/' && !project },
    { href: '#/inbox', icon: Inbox, label: 'Inbox', count: app.inbox, active: router.path === '/inbox', loud: true },
    { href: '#/drafts', icon: FilePen, label: 'Drafts', count: app.info?.drafts ?? 0, active: router.path === '/drafts', loud: true },
    { href: '#/discover', icon: Compass, label: 'Discover', count: app.recommended, active: router.path === '/discover' },
    { href: '#/map', icon: Network, label: 'Map', count: 0, active: router.path === '/map' },
    { href: '#/people', icon: Users, label: 'People', count: 0, active: router.path === '/people' },
    { href: '#/health', icon: HeartPulse, label: 'Health', count: 0, active: router.path === '/health' },
    // A server's changes and deletion requests, for those who may change the library.
    ...(app.me?.server && app.can.edit
      ? [{ href: '#/activity', icon: History, label: 'Activity', count: app.pending, active: router.path === '/activity', loud: true }]
      : []),
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

  <div class="mt-6 flex items-center px-5 pb-1.5">
    <span class="flex-1 text-xs font-medium tracking-wide text-faint uppercase">Projects</span>
    {#if app.can.add}
      <button class="cursor-pointer rounded-md p-1 text-faint hover:bg-hover hover:text-foreground" onclick={() => (creating = !creating)} aria-label="New project" title="New project">
        <FolderPlus class="size-3.5" />
      </button>
    {/if}
  </div>
  <div class="min-h-0 flex-1 space-y-0.5 overflow-y-auto px-3">
    {#if creating}
      <form onsubmit={create} class="space-y-1.5 px-2.5 pb-2">
        <!-- svelte-ignore a11y_autofocus -->
        <input bind:value={newName} autofocus placeholder="New project" onkeydown={(e) => e.key === 'Escape' && (creating = false)} class="h-7 w-full rounded-md border bg-transparent px-2 text-sm outline-none placeholder:text-faint focus:border-accent" />
        <div class="flex items-center gap-2">
          {#if app.me?.user}
            <label class="flex flex-1 cursor-pointer items-center gap-1.5 text-xs text-muted" title="Only you see it, on all your machines">
              <input type="checkbox" bind:checked={newPrivate} class="accent-accent" />Private
            </label>
          {:else}
            <span class="flex-1"></span>
          {/if}
          <Button size="sm" variant="ghost" type="button" onclick={() => (creating = false)}>Cancel</Button>
          <Button size="sm" variant="primary" type="submit" disabled={!newName.trim()}>Create</Button>
        </div>
      </form>
    {/if}
    {#each app.projects as p (p.name)}
      {#if renaming === p.name}
        <form onsubmit={(e) => rename(e, p)} class="px-2.5 py-0.5">
          <!-- svelte-ignore a11y_autofocus -->
          <input bind:value={renamed} autofocus onkeydown={(e) => e.key === 'Escape' && (renaming = null)} onblur={() => (renaming = null)} aria-label="New name for {p.name}" class="h-7 w-full rounded-md border bg-transparent px-2 text-sm outline-none focus:border-accent" />
        </form>
      {:else}
        <div class="group relative">
          <a
            href={href('/', { project: p.name })}
            onclick={onnavigate}
            class="flex items-center gap-2.5 rounded-lg px-2.5 py-1.5 text-sm transition {project === p.name ? 'bg-hover font-medium text-foreground' : 'text-muted hover:bg-hover/60 hover:text-foreground'}"
            title={p.private ? 'Private: only you see it' : undefined}
          >
            {#if p.private}<FolderLock class="size-4" />{:else}<Folder class="size-4" />{/if}
            <span class="flex-1 truncate">{p.name}</span>
            <span class="text-xs text-faint {mayChange(p) || mayDelete(p) ? 'group-focus-within:invisible group-hover:invisible' : ''}">{p.papers.length}</span>
          </a>
          {#if mayChange(p) || mayDelete(p)}
            <div class="absolute inset-y-0 right-1 hidden items-center gap-0.5 group-focus-within:flex group-hover:flex">
              {#if mayChange(p)}
                <button class="cursor-pointer rounded p-1 text-faint hover:bg-hover hover:text-foreground" onclick={() => ((renamed = p.name), (renaming = p.name))} aria-label="Rename {p.name}" title="Rename">
                  <Pencil class="size-3" />
                </button>
                {#if app.me?.user}
                  <button
                    class="cursor-pointer rounded p-1 text-faint hover:bg-hover hover:text-foreground"
                    onclick={() => projectCall(() => api.changeProject(p.name, { private: !p.private }), p.private ? `${p.name} is public now` : `${p.name} is private now`)}
                    aria-label={p.private ? `Make ${p.name} public` : `Make ${p.name} private`}
                    title={p.private ? 'Make it public' : 'Make it private'}
                  >
                    {#if p.private}<LockOpen class="size-3" />{:else}<Lock class="size-3" />{/if}
                  </button>
                {/if}
              {/if}
              {#if mayDelete(p)}
                <button class="cursor-pointer rounded p-1 text-faint hover:bg-hover hover:text-failed" onclick={() => ((deleting = p), (confirming = true))} aria-label="{asks(p) ? 'Ask to delete' : 'Delete'} {p.name}" title={asks(p) ? 'Ask to delete' : 'Delete'}>
                  <Trash2 class="size-3" />
                </button>
              {/if}
            </div>
          {/if}
        </div>
      {/if}
    {:else}
      {#if !creating}<p class="px-2.5 text-xs text-faint">None yet. Start one with + above, or put a paper in one from its page.</p>{/if}
    {/each}
  </div>

  <Dialog bind:open={confirming} title="{deleting && asks(deleting) ? 'Ask to delete' : 'Delete'} {deleting?.name}?">
    {#if deleting && asks(deleting)}
      <p class="text-sm text-muted">An admin other than you decides. Until then the project stays. Its papers stay in the library either way.</p>
    {:else}
      <p class="text-sm text-muted">This deletes the project; its papers stay in the library.</p>
    {/if}
    {#snippet footer()}
      <Button variant="ghost" onclick={() => (confirming = false)}>Cancel</Button>
      <Button variant="danger" onclick={removeProject}><Trash2 class="size-4" />{deleting && asks(deleting) ? 'Ask to delete' : 'Delete'}</Button>
    {/snippet}
  </Dialog>

  <div class="space-y-2 border-t px-3 py-3">
    {#if me?.user}
      <div class="flex items-center gap-1">
        <div class="flex min-w-0 flex-1 items-center gap-2 px-2.5 text-sm" title={me.user}>
          <CircleUser class="size-4 shrink-0 text-muted" />
          <span class="truncate">{me.name ?? me.user}</span>
          <Badge tone={me.role === 'admin' ? 'accent' : 'neutral'} title={me.role === 'admin' ? 'Reverts changes and decides on deletions' : 'Adds and changes papers, asks to delete'}>
            {me.role === 'admin' ? 'admin' : 'member'}
          </Badge>
        </div>
        <a href="#/tokens" onclick={onnavigate} class="rounded-lg p-2 text-muted hover:bg-hover hover:text-foreground {router.path === '/tokens' ? 'bg-hover text-foreground' : ''}" aria-label="Tokens" title="Tokens">
          <KeyRound class="size-4" />
        </a>
        <button class="cursor-pointer rounded-lg p-2 text-muted hover:bg-hover hover:text-foreground" onclick={logout} aria-label="Log out" title="Log out">
          <LogOut class="size-4" />
        </button>
      </div>
    {:else if me?.role === 'anonymous' && me.login}
      <button class="flex w-full cursor-pointer items-center gap-2 rounded-lg px-2.5 py-1.5 text-sm text-muted hover:bg-hover hover:text-foreground" onclick={() => (location.href = api.loginUrl())}>
        <LogIn class="size-4" />Log in to change the library
      </button>
    {/if}
    {#if app.info?.embeddings}
      {@const e = app.info.embeddings}
      <div class="flex items-center gap-2 px-2.5 text-xs text-muted" title={'error' in e ? e.error : `${e.model} on ${e.url}`}>
        <Sparkles class="size-3.5 {'error' in e ? 'text-failed' : 'text-ready'}" />
        <span class="truncate">{'error' in e ? 'Semantic search is down' : `Semantic search: ${e.model}`}</span>
      </div>
    {/if}
    <div class="flex items-center gap-1">
      {#if app.info?.remote}
        <button class="flex flex-1 cursor-pointer items-center gap-2 rounded-lg px-2.5 py-1.5 text-sm text-muted hover:bg-hover hover:text-foreground" onclick={sync} disabled={syncing || !app.can.edit} title={app.can.edit ? undefined : 'Log in to change the library'}>
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
