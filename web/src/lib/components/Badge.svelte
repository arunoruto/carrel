<script lang="ts">
  import type { Snippet } from 'svelte';

  let {
    tone = 'neutral',
    title,
    children,
  }: { tone?: 'neutral' | 'ready' | 'failed' | 'wanted' | 'warning' | 'accent'; title?: string; children: Snippet } =
    $props();

  const tones = {
    neutral: 'bg-hover text-muted',
    ready: 'bg-ready/15 text-ready',
    failed: 'bg-failed/15 text-failed',
    wanted: 'bg-wanted/20 text-wanted',
    warning: 'bg-warning/15 text-warning',
    accent: 'bg-accent/15 text-accent',
  };
</script>

<span class="inline-flex items-center gap-1 rounded-md px-1.5 py-0.5 text-[0.7rem] font-medium whitespace-nowrap {tones[tone]}" {title}>
  {@render children()}
</span>
