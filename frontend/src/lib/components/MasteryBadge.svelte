<script lang="ts">
  let {
    consecutiveCorrect,
    required = 3,
  }: {
    consecutiveCorrect: number;
    required?: number;
  } = $props();

  let bgColor = $derived(
    consecutiveCorrect === 0
      ? 'bg-red-500'
      : consecutiveCorrect === 1
        ? 'bg-yellow-500'
        : 'bg-orange-500'
  );
</script>

<span class="inline-flex items-center px-2 py-0.5 rounded-full text-xs font-semibold text-white {bgColor}">
  Progress: {consecutiveCorrect}/{required} ✓
</span>
