<script lang="ts">
  import { onMount } from 'svelte';
  import { getAuth } from '$lib/auth.svelte';
  import { goto } from '$app/navigation';
  import { api } from '$lib/api';
  import QuestionCard from '$lib/components/QuestionCard.svelte';
  import CategoryFilter from '$lib/components/CategoryFilter.svelte';

  const auth = getAuth();

  $effect(() => {
    if (!auth.loading && !auth.user) goto('/login');
  });

  // --- Types ---
  interface MasteredQuestion {
    id: number;
    question: string;
    answer: string;
    category: string;
    classifier_category: string;
    clue_value: number | null;
    round: number | null;
    air_date: string | null;
    mastered_at: string;
    total_mastered: number;
  }

  // --- State ---
  let masteredQuestion = $state<MasteredQuestion | null>(null);
  let categories = $state<Array<{ name: string; count: number }>>([]);
  let selectedCategory = $state('all');
  let loading = $state(true);
  let error = $state('');
  let showAnswer = $state(false);
  let resetting = $state(false);
  let showResetConfirm = $state(false);

  let masteredDate = $derived(
    masteredQuestion?.mastered_at
      ? new Date(masteredQuestion.mastered_at).toLocaleDateString()
      : ''
  );

  // --- API ---
  async function fetchMastered() {
    loading = true;
    error = '';
    showAnswer = false;
    try {
      const params = new URLSearchParams();
      if (selectedCategory !== 'all') params.set('category', selectedCategory);
      masteredQuestion = await api.get(`/api/mastered?${params}`);
    } catch (err: any) {
      if ((err as any)?.status === 404) {
        masteredQuestion = null;
      } else {
        error = err?.message ?? 'Failed to load mastered question';
      }
    } finally {
      loading = false;
    }
  }

  async function handleResetMastery() {
    if (!masteredQuestion) return;
    resetting = true;
    try {
      await api.post('/api/mastery/reset', { questionId: masteredQuestion.id });
      showResetConfirm = false;
      await fetchMastered();
    } catch (err: any) {
      error = err?.message ?? 'Failed to reset mastery';
    } finally {
      resetting = false;
    }
  }

  async function handleCategoryChange(value: string) {
    selectedCategory = value;
    await fetchMastered();
  }

  // --- Keyboard shortcuts ---
  function handleKeydown(e: KeyboardEvent) {
    if (e.target instanceof HTMLInputElement || e.target instanceof HTMLSelectElement) return;
    if (e.code === 'Space' && !showAnswer && masteredQuestion) {
      e.preventDefault();
      showAnswer = true;
    }
  }

  // --- Mount ---
  onMount(async () => {
    try {
      const cats = await api.get('/api/categories');
      categories = cats ?? [];
    } catch {
      // Non-critical
    }
    await fetchMastered();
  });
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="min-h-screen bg-gray-50 py-6 px-4">
  <div class="max-w-2xl mx-auto flex flex-col gap-4">

    <!-- Header -->
    <div class="bg-green-700 rounded-xl px-6 py-4 text-white">
      <h1 class="text-2xl font-bold">Mastered Questions Review</h1>
      {#if masteredQuestion?.total_mastered}
        <p class="text-green-200 text-sm mt-1">{masteredQuestion.total_mastered} mastered question{masteredQuestion.total_mastered === 1 ? '' : 's'}</p>
      {/if}
    </div>

    <!-- Category filter -->
    <div class="bg-white rounded-xl shadow-sm px-5 py-4">
      <p class="block text-xs font-semibold text-gray-500 uppercase tracking-wide mb-1">Category</p>
      <CategoryFilter
        {categories}
        selected={selectedCategory}
        onchange={handleCategoryChange}
      />
    </div>

    {#if error}
      <div class="px-4 py-3 bg-red-50 border border-red-200 text-red-700 rounded-lg text-sm">
        {error}
        <button onclick={() => (error = '')} class="ml-2 underline text-red-500">Dismiss</button>
      </div>
    {/if}

    {#if loading}
      <div class="flex justify-center py-16">
        <div class="animate-spin rounded-full h-12 w-12 border-b-2 border-green-700"></div>
      </div>
    {:else if !masteredQuestion}
      <div class="text-center py-16 text-gray-500">
        <p class="text-lg font-medium">No mastered questions yet!</p>
        <p class="text-sm mt-1">Answer questions correctly {3} times in a row to master them.</p>
      </div>
    {:else}
      <div class="min-h-[420px]">
        <QuestionCard
          clue={masteredQuestion.answer}
          answer={masteredQuestion.question}
          category={masteredQuestion.category}
          classifierCategory={masteredQuestion.classifier_category}
          clueValue={masteredQuestion.clue_value}
          round={masteredQuestion.round}
          airDate={masteredQuestion.air_date}
          {showAnswer}
          cardBgColor="bg-green-700"
          cardTextColor="text-yellow-300"
          onRevealAnswer={() => { showAnswer = true; }}
          onCorrect={() => fetchMastered()}
          onIncorrect={() => fetchMastered()}
        >
          {#snippet badge()}
            <span class="inline-flex items-center px-3 py-1 rounded-full text-xs font-semibold bg-green-900 text-green-100">
              Mastered {masteredDate}
            </span>
          {/snippet}

          {#snippet additionalActions()}
            <button
              onclick={() => { showResetConfirm = true; }}
              class="w-full py-2 rounded-lg bg-white/10 hover:bg-white/20 border border-white/20 text-white/80 text-sm font-medium transition-colors"
            >
              Reset Mastery
            </button>
          {/snippet}
        </QuestionCard>
      </div>

      <!-- Next question button -->
      <button
        onclick={() => fetchMastered()}
        class="w-full py-3 rounded-xl bg-green-700 hover:bg-green-800 text-white font-semibold text-lg transition-colors"
      >
        Next Question
      </button>

      <!-- Keyboard hint -->
      <p class="text-center text-xs text-gray-400">
        {#if !showAnswer}
          Press <kbd class="px-1.5 py-0.5 bg-gray-100 rounded border border-gray-300 font-mono">Space</kbd> to reveal answer
        {/if}
      </p>
    {/if}

  </div>
</div>

<!-- Reset mastery confirmation dialog -->
{#if showResetConfirm}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 px-4">
    <div class="w-full max-w-sm rounded-2xl bg-white shadow-2xl p-6 flex flex-col gap-4">
      <h2 class="text-lg font-bold text-gray-800">Reset Mastery?</h2>
      <p class="text-sm text-gray-600">
        This will reset the mastery status for this question. You'll need to answer it correctly
        multiple times again to re-master it.
      </p>
      <div class="flex gap-3">
        <button
          onclick={() => { showResetConfirm = false; }}
          class="flex-1 py-2.5 rounded-xl border border-gray-300 text-gray-700 text-sm font-medium hover:bg-gray-50 transition-colors"
        >
          Cancel
        </button>
        <button
          onclick={handleResetMastery}
          disabled={resetting}
          class="flex-1 py-2.5 rounded-xl bg-red-500 text-white text-sm font-semibold hover:bg-red-600 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
        >
          {resetting ? 'Resetting...' : 'Reset Mastery'}
        </button>
      </div>
    </div>
  </div>
{/if}
