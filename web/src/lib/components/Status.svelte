<script lang="ts">
  import Badge from './Badge.svelte';

  // The same statuses as the TUI's.
  // `quiet` leaves out "ready", the usual case, in long lists.
  let { pdf, text, drafts, quiet = false }: { pdf: boolean; text: boolean; drafts: unknown[]; quiet?: boolean } = $props();
</script>

{#if drafts.length}
  <Badge tone="warning" title="An AI proposal waits for you in Drafts">draft</Badge>
{/if}
{#if !pdf}
  <Badge tone="wanted" title="Metadata only; attach its PDF">no pdf</Badge>
{:else if !text}
  <Badge tone="failed" title="Its text couldn't be extracted yet">no text</Badge>
{:else if !quiet}
  <Badge tone="ready" title="Text extracted: an AI can read it">ready</Badge>
{/if}
