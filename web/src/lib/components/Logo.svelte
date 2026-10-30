<script lang="ts">
  let { class: extra = 'size-7' }: { class?: string } = $props();
  // The TUI's book spines.
  const spines = [
    ['#f38ba8', 9, 20],
    ['#fab387', 5, 24],
    ['#f9e2af', 7, 22],
    ['#a6e3a1', 11, 18],
  ] as const;
</script>

<svg class={extra} viewBox="0 0 32 32" aria-hidden="true">
  {#each spines as [color, y, h], i}
    <rect x={3 + i * 5} {y} width="4" height={h} rx="1" fill={color} />
  {/each}
  <rect x="23" y="4" width="4" height="25" rx="1" fill="#89b4fa" transform="rotate(8 25 16)" />
</svg>
