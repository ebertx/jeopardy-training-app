<script lang="ts">
  import type { PavlovProgress } from '$lib/stats';
  import { fmtDate } from '$lib/stats';

  let { progress }: { progress: PavlovProgress } = $props();

  // green: trailing pace meets requirement; amber: within 20% below; red otherwise.
  let paceClass = $derived.by(() => {
    if (progress.requiredPerDay === 0) return 'text-green-600';
    const ratio = progress.trailingPerDay / progress.requiredPerDay;
    return ratio >= 1 ? 'text-green-600' : ratio >= 0.8 ? 'text-amber-500' : 'text-red-500';
  });
  let remaining = $derived(Math.max(0, progress.deckTotal - progress.touched));
</script>

<div class="flex-1 min-w-[260px] bg-white rounded-xl shadow p-6 border-2 border-jeopardy-gold">
  <p class="text-sm font-medium text-gray-500 mb-1">Deck progress</p>
  <p class="text-3xl font-bold text-jeopardy-blue">
    {progress.touched.toLocaleString()}<span class="text-gray-400 text-xl"> / {progress.deckTotal.toLocaleString()}</span>
    <span class="text-base font-semibold text-gray-500 ml-2">{progress.touchedPct.toFixed(0)}%</span>
  </p>
  <div class="mt-2 h-2 rounded-full bg-gray-100 overflow-hidden">
    <div class="h-full bg-jeopardy-gold rounded-full" style="width: {Math.min(100, progress.touchedPct)}%"></div>
  </div>
  {#if remaining === 0}
    <p class="text-xs text-green-600 mt-2 font-semibold">Deck complete.</p>
  {:else if progress.pastTarget}
    <p class="text-xs text-red-500 mt-2">
      Target {fmtDate(progress.targetDate)} passed — {remaining.toLocaleString()} cards left.
      <a href="/settings" class="text-jeopardy-blue hover:underline">Change target</a>
    </p>
  {:else}
    <p class="text-xs text-gray-400 mt-2">
      Need <span class="font-semibold {paceClass}">{progress.requiredPerDay}/day</span>
      to finish by <a href="/settings" class="text-jeopardy-blue hover:underline">{fmtDate(progress.targetDate)}</a>
      · trailing <span class="font-semibold {paceClass}">{progress.trailingPerDay.toFixed(0)}/day</span>
      {#if progress.projectedFinish}
        · projected {fmtDate(progress.projectedFinish)}
      {:else}
        · no new cards in 14 days
      {/if}
      {#if progress.daysAhead !== null}
        · <span class="font-semibold {paceClass}">{Math.abs(progress.daysAhead)} days {progress.daysAhead >= 0 ? 'ahead' : 'behind'}</span>
      {/if}
    </p>
  {/if}
  {#if progress.hooksTotal > 0}
    <p class="text-xs text-gray-400 mt-1">
      Hook coverage <span class="font-semibold text-gray-600">{progress.hooksSeen.toLocaleString()} / {progress.hooksTotal.toLocaleString()}</span>
      · {Math.round((progress.hooksSeen / progress.hooksTotal) * 100)}% of the angles on cards you've touched
    </p>
  {/if}
</div>
