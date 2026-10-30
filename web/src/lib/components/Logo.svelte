<script lang="ts">
  // assets/logo.svg: a reading nook with three books, the nook in the text colour.
  let { class: extra = 'size-7' }: { class?: string } = $props();
</script>

<svg class={extra} viewBox="0 0 64 64" fill="none" aria-hidden="true">
  <path d="M9 57V29a23 23 0 0 1 46 0v28M5 57h54" stroke="currentColor" stroke-width="7" stroke-linecap="round" />
  <rect x="17" y="30" width="8" height="24" rx="2" fill="#e8586e" />
  <rect x="27" y="23" width="8" height="31" rx="2" fill="#f2a33a" />
  <rect x="36.5" y="27" width="8" height="27" rx="2" fill="#4c7ef0" transform="rotate(11 36.5 54)" />
</svg>
