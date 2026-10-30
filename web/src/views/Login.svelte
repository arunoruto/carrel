<script lang="ts">
  import { KeyRound } from '@lucide/svelte';
  import Logo from '../lib/components/Logo.svelte';
</script>

<div class="flex h-full items-center justify-center p-6">
  <div class="w-full max-w-md rounded-2xl border bg-card p-8 text-center shadow-lg">
    <Logo class="mx-auto size-12" />
    <h1 class="mt-4 text-xl font-semibold">Open carrel from its link</h1>
    <p class="mt-2 text-sm text-muted">
      This page only shows your library to the browser that opened the link <code class="rounded bg-hover px-1">carrel web</code> printed, so that other websites can't read it.
    </p>
    <div class="mt-5 flex items-center gap-2 rounded-lg bg-hover px-3 py-2.5 text-left font-mono text-xs">
      <KeyRound class="size-4 shrink-0 text-faint" />
      <span>$ carrel web</span>
    </div>
    <p class="mt-3 text-xs text-faint">Then open the http://127.0.0.1:…/?token=… link it prints.</p>
  </div>
</div>
