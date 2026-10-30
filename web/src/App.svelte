<script lang="ts">
  import { Menu } from '@lucide/svelte';
  import DropZone from './lib/components/DropZone.svelte';
  import Sidebar from './lib/components/Sidebar.svelte';
  import Toasts from './lib/components/Toasts.svelte';
  import { router } from './lib/router.svelte';
  import { app } from './lib/state.svelte';
  import Discover from './views/Discover.svelte';
  import Drafts from './views/Drafts.svelte';
  import Inbox from './views/Inbox.svelte';
  import Library from './views/Library.svelte';
  import Login from './views/Login.svelte';
  import Paper from './views/Paper.svelte';

  let menu = $state(false);
  app.refresh();
  // Downloads arrive while the tab is open; look again when it comes back into view.
  const visible = () => document.visibilityState === 'visible' && app.refresh();

  $effect(() => {
    void router.path;
    menu = false;
  });
</script>

<svelte:document onvisibilitychange={visible} />

{#if app.loggedOut}
  <Login />
{:else}
  <div class="flex h-full">
    <div class="hidden lg:block"><Sidebar /></div>
    {#if menu}
      <div class="fixed inset-0 z-30 flex lg:hidden">
        <Sidebar onnavigate={() => (menu = false)} />
        <button class="flex-1 bg-black/40" onclick={() => (menu = false)} aria-label="Close the menu"></button>
      </div>
    {/if}
    <main class="flex min-w-0 flex-1 flex-col">
      <div class="flex items-center gap-2 border-b px-4 py-2 lg:hidden">
        <button class="cursor-pointer rounded-md p-1.5 hover:bg-hover" onclick={() => (menu = true)} aria-label="Menu"><Menu class="size-5" /></button>
        <span class="font-semibold">carrel</span>
      </div>
      <div class="min-h-0 flex-1 overflow-y-auto">
        {#if router.parts[0] === 'paper' && router.parts[1]}
          <Paper id={router.parts[1]} reading={router.parts[2] === 'read'} />
        {:else if router.path === '/inbox'}
          <Inbox />
        {:else if router.path === '/drafts'}
          <Drafts />
        {:else if router.path === '/discover'}
          <Discover />
        {:else}
          {#key router.params.toString()}<Library />{/key}
        {/if}
      </div>
    </main>
  </div>
  <DropZone />
{/if}
<Toasts />
