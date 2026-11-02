<script lang="ts">
  import { Copy, KeyRound, Trash2 } from '@lucide/svelte';
  import { api, type Token } from '../lib/api';
  import Button from '../lib/components/Button.svelte';
  import Dialog from '../lib/components/Dialog.svelte';
  import { toasts } from '../lib/toast.svelte';

  // Personal tokens for the machines and AIs that sync with this server or use its MCP: one per machine,
  // so a lost laptop is one revoke.
  let tokens = $state<Token[]>([]);
  let name = $state('');
  let busy = $state(false);
  let created = $state<(Token & { token: string }) | null>(null);
  let revoking = $state<Token | null>(null);
  let confirming = $state(false);

  const load = () =>
    api
      .tokens()
      .then((t) => (tokens = t))
      .catch((e) => toasts.error(e, 'Cannot load your tokens'));
  load();

  async function create(e: SubmitEvent) {
    e.preventDefault();
    busy = true;
    try {
      created = await api.createToken(name.trim());
      name = '';
      load();
    } catch (e) {
      toasts.error(e, 'Cannot create the token');
    }
    busy = false;
  }

  async function revoke() {
    const t = revoking!;
    confirming = false;
    try {
      await api.revokeToken(t.id);
      toasts.show('success', `Revoked ${t.name}`);
      load();
    } catch (e) {
      toasts.error(e, 'Cannot revoke the token');
    }
  }

  const config = $derived(created && `[remote]\ntype = "carrel"\nurl = "${location.origin}"\ntoken = "${created.token}"`);
  const mcp = $derived(created && `claude mcp add --transport http carrel ${location.origin}/mcp --header "Authorization: Bearer ${created.token}"`);
</script>

{#snippet copyable(text: string)}
  <div class="relative">
    <pre class="overflow-x-auto rounded-lg bg-hover px-3 py-2.5 pr-10 font-mono text-xs">{text}</pre>
    <button class="absolute top-2 right-2 cursor-pointer rounded-md p-1 text-muted hover:bg-card hover:text-foreground" title="Copy" onclick={() => navigator.clipboard.writeText(text)}>
      <Copy class="size-3.5" />
    </button>
  </div>
{/snippet}

<div class="mx-auto flex max-w-3xl flex-col gap-6 px-6 py-6 lg:px-10 lg:py-8">
  <header>
    <h1 class="text-2xl font-semibold tracking-tight">Tokens</h1>
    <p class="mt-1 text-sm text-muted">For the machines that sync with this server and the AIs that use it. Make one per machine, so you can revoke one alone.</p>
  </header>

  <form class="flex gap-2" onsubmit={create}>
    <input
      bind:value={name}
      required
      placeholder="What it's for: laptop, desktop, Claude…"
      class="h-9 min-w-0 flex-1 rounded-lg border bg-background px-3 text-sm outline-none placeholder:text-faint focus:border-accent focus:ring-2 focus:ring-accent/20"
    />
    <Button variant="primary" type="submit" {busy} disabled={!name.trim()}><KeyRound class="size-4" />Create</Button>
  </form>

  {#if created}
    <section class="space-y-3 rounded-xl border border-accent/40 bg-accent/5 p-4 text-sm">
      <p><span class="font-medium">{created.name}</span>: copy it now, it can't be shown again.</p>
      {@render copyable(created.token)}
      <p class="text-muted">To sync a library through this server, in <code class="rounded bg-hover px-1">~/.config/carrel/config.toml</code>:</p>
      {@render copyable(config!)}
      <p class="text-muted">For Claude Code (other MCP clients take the same URL and header):</p>
      {@render copyable(mcp!)}
    </section>
  {/if}

  <section class="overflow-hidden rounded-xl border">
    {#each tokens as t}
      <div class="flex items-center gap-3 border-b px-4 py-3 text-sm last:border-b-0">
        <KeyRound class="size-4 text-faint" />
        <span class="flex-1 font-medium">{t.name}</span>
        <span class="text-xs text-faint">since {t.created}</span>
        <Button variant="ghost" size="icon" title="Revoke" onclick={() => ((revoking = t), (confirming = true))}><Trash2 class="size-4" /></Button>
      </div>
    {:else}
      <p class="px-4 py-6 text-center text-sm text-faint">No tokens yet.</p>
    {/each}
  </section>
</div>

<Dialog bind:open={confirming} title="Revoke this token?">
  <p class="text-sm">Whatever uses <span class="font-medium">{revoking?.name}</span> can't sync or reach the library with it anymore.</p>
  {#snippet footer()}
    <Button onclick={() => (confirming = false)}>Keep it</Button>
    <Button variant="danger" onclick={revoke}>Revoke</Button>
  {/snippet}
</Dialog>
