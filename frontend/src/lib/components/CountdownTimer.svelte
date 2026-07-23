<script lang="ts">
  // Display-only countdown: ticks to 0 and sits there. No callbacks, no
  // logging — the user self-scores against it (spec 2026-07-23 §3).
  let { seconds = 8, running = true, resetKey = 0 }: { seconds?: number; running?: boolean; resetKey?: unknown } = $props();
  let remaining = $state(seconds);
  $effect(() => {
    void resetKey; // re-run on card change
    remaining = seconds;
    if (!running) return;
    const iv = setInterval(() => {
      remaining = Math.max(0, remaining - 1);
      if (remaining === 0) clearInterval(iv);
    }, 1000);
    return () => clearInterval(iv);
  });
</script>

<span class="text-xs tabular-nums {remaining === 0 ? 'text-red-300' : 'text-gray-400'}">{remaining}s</span>
