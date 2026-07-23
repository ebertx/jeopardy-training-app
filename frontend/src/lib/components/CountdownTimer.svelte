<script lang="ts">
  // Display-only countdown: ticks to 0 and sits there. No callbacks, no
  // logging — the user self-scores against it (spec 2026-07-23 §3).
  let { seconds = 8, running = true, resetKey = 0 }: { seconds?: number; running?: boolean; resetKey?: unknown } = $props();
  let remaining = $state(seconds);
  // Reset only on card change — a running→false flip (reveal) must FREEZE the
  // display at its current value, not reset it.
  $effect(() => {
    void resetKey;
    remaining = seconds;
  });
  $effect(() => {
    if (!running) return;
    const iv = setInterval(() => {
      remaining = Math.max(0, remaining - 1);
      if (remaining === 0) clearInterval(iv);
    }, 1000);
    return () => clearInterval(iv);
  });
</script>

<span
  title="Anytime Test pace — aim to answer before it hits 0. Display only."
  class="text-xs tabular-nums {remaining === 0 ? 'text-red-300' : 'text-gray-400'}">{remaining}s</span>
