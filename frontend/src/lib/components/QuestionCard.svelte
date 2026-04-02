<script lang="ts">
  import type { Snippet } from 'svelte';

  let {
    clue,
    answer,
    category,
    classifierCategory,
    clueValue = null,
    round = null,
    airDate = null,
    showAnswer,
    onRevealAnswer,
    onCorrect,
    onIncorrect,
    badge,
    additionalActions,
    cardBgColor = 'bg-jeopardy-blue',
    cardTextColor = 'text-jeopardy-gold',
    submitting = false,
  }: {
    clue: string;
    answer: string;
    category: string;
    classifierCategory: string;
    clueValue?: number | null;
    round?: number | null;
    airDate?: string | null;
    showAnswer: boolean;
    onRevealAnswer: () => void;
    onCorrect: () => void;
    onIncorrect: () => void;
    badge?: Snippet;
    additionalActions?: Snippet;
    cardBgColor?: string;
    cardTextColor?: string;
    submitting?: boolean;
  } = $props();
</script>

<div class="flex flex-col h-full {cardBgColor} rounded-2xl shadow-xl overflow-hidden">
  <!-- Badge slot -->
  {#if badge}
    <div class="px-6 pt-4">
      {@render badge()}
    </div>
  {/if}

  <!-- Category header -->
  <div class="px-6 pt-5 pb-2">
    <p class="text-xs font-bold uppercase tracking-widest text-white/60">{category}</p>
  </div>

  <!-- Clue text -->
  <div class="flex-1 flex items-center justify-center px-6 py-6">
    <p class="text-center text-2xl sm:text-3xl font-bold leading-snug {cardTextColor}">
      {clue}
    </p>
  </div>

  <!-- Answer area -->
  <div class="px-6 pb-4">
    {#if !showAnswer}
      <button
        onclick={onRevealAnswer}
        class="w-full py-3 rounded-xl bg-white/10 hover:bg-white/20 text-white font-semibold text-lg transition-colors border border-white/20"
      >
        Show Answer
      </button>
    {:else}
      <!-- Answer box -->
      <div class="bg-white rounded-xl px-5 py-4 mb-4 text-center">
        <p class="text-gray-900 font-bold text-xl">{answer}</p>
      </div>

      <!-- Correct / Incorrect buttons -->
      <div class="flex gap-3">
        <button
          onclick={onIncorrect}
          disabled={submitting}
          class="flex-1 py-3 rounded-xl bg-red-500 hover:bg-red-600 disabled:opacity-50 disabled:cursor-not-allowed text-white font-semibold text-lg transition-colors"
        >
          ← Incorrect
        </button>
        <button
          onclick={onCorrect}
          disabled={submitting}
          class="flex-1 py-3 rounded-xl bg-green-500 hover:bg-green-600 disabled:opacity-50 disabled:cursor-not-allowed text-white font-semibold text-lg transition-colors"
        >
          Correct →
        </button>
      </div>

      <!-- Additional actions slot -->
      {#if additionalActions}
        <div class="mt-3">
          {@render additionalActions()}
        </div>
      {/if}
    {/if}
  </div>

  <!-- Footer bar -->
  <div class="px-6 py-3 bg-black/20 flex items-center gap-3 text-xs text-white/70 flex-wrap">
    <span class="font-semibold uppercase tracking-wide">{classifierCategory}</span>
    {#if clueValue !== null}
      <span>•</span>
      <span>${clueValue.toLocaleString()}</span>
    {/if}
    {#if airDate}
      <span>•</span>
      <span>{airDate}</span>
    {/if}
  </div>
</div>
