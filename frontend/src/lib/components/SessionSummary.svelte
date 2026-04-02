<script lang="ts">
  let {
    summary,
    onclose,
  }: {
    summary: {
      total: number;
      correct: number;
      accuracy: number;
      startedAt: string;
      completedAt: string;
    };
    onclose: () => void;
  } = $props();

  function formatTime(iso: string): string {
    return new Date(iso).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  }
</script>

<!-- Overlay -->
<div
  class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 px-4"
  role="dialog"
  aria-modal="true"
  aria-label="Session summary"
>
  <div class="w-full max-w-sm rounded-2xl bg-white shadow-2xl overflow-hidden">
    <!-- Header -->
    <div class="bg-jeopardy-blue px-6 py-5 text-center">
      <h2 class="text-xl font-bold text-jeopardy-gold">Session Complete!</h2>
    </div>

    <!-- Body -->
    <div class="px-6 py-6 flex flex-col items-center gap-4">
      <!-- Accuracy big number -->
      <div class="text-center">
        <p class="text-6xl font-extrabold {summary.accuracy >= 75 ? 'text-green-600' : summary.accuracy >= 50 ? 'text-amber-500' : 'text-red-500'}">
          {summary.accuracy.toFixed(1)}%
        </p>
        <p class="text-sm text-gray-500 mt-1">Accuracy</p>
      </div>

      <!-- Correct / Total -->
      <div class="text-center">
        <p class="text-2xl font-bold text-gray-800">{summary.correct} / {summary.total}</p>
        <p class="text-sm text-gray-500">Correct answers</p>
      </div>

      <!-- Time range -->
      <div class="text-center text-sm text-gray-500">
        <p>{formatTime(summary.startedAt)} – {formatTime(summary.completedAt)}</p>
      </div>
    </div>

    <!-- Footer -->
    <div class="px-6 pb-6">
      <button
        onclick={onclose}
        class="w-full py-3 rounded-xl bg-jeopardy-blue hover:bg-blue-800 text-white font-semibold text-lg transition-colors"
      >
        Close
      </button>
    </div>
  </div>
</div>
