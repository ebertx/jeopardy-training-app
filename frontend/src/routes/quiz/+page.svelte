<script lang="ts">
  import { onMount } from 'svelte';
  import { getAuth } from '$lib/auth.svelte';
  import { goto } from '$app/navigation';
  import { api } from '$lib/api';
  import QuestionCard from '$lib/components/QuestionCard.svelte';
  import CategoryFilter from '$lib/components/CategoryFilter.svelte';
  import SessionSummary from '$lib/components/SessionSummary.svelte';

  const auth = getAuth();

  $effect(() => {
    if (!auth.loading && !auth.user) goto('/login');
  });

  // --- State ---
  let question = $state<any>(null);
  let prefetchedQuestion = $state<any>(null);
  let showAnswer = $state(false);
  let loading = $state(true);
  let error = $state('');
  let sessionId = $state<number | null>(null);
  let categories = $state<Array<{ name: string; count: number }>>([]);
  let selectedCategory = $state('all');
  let runningStats = $state({ total: 0, correct: 0 });
  let gameTypeFilters = $state<string[]>([]);
  let showSessionSummary = $state(false);
  let sessionSummary = $state<any>(null);
  let submitting = $state(false);
  let showEndConfirm = $state(false);

  // Derived accuracy
  let accuracy = $derived(
    runningStats.total > 0
      ? Math.round((runningStats.correct / runningStats.total) * 100)
      : 0
  );

  // --- API helpers ---
  function buildQuizParams(): URLSearchParams {
    const params = new URLSearchParams();
    if (selectedCategory !== 'all') params.set('category', selectedCategory);
    if (gameTypeFilters.length > 0) params.set('gameTypes', gameTypeFilters.join(','));
    return params;
  }

  async function fetchQuestion() {
    loading = true;
    error = '';
    try {
      const params = buildQuizParams();
      const q = await api.get(`/api/quiz/random?${params}`);
      question = q;
    } catch (err: any) {
      error = err?.message ?? 'Failed to load question';
    } finally {
      loading = false;
    }
  }

  function prefetchNextQuestion() {
    const params = buildQuizParams();
    fetch(`/api/quiz/random?${params}`, { credentials: 'same-origin' })
      .then((r) => r.json())
      .then((q) => {
        prefetchedQuestion = q;
      })
      .catch(() => {});
  }

  async function handleAnswer(correct: boolean) {
    if (submitting) return;
    submitting = true;
    try {
      const result = await api.post('/api/quiz/submit', {
        questionId: question.id,
        correct,
        sessionId,
        isReviewSession: false,
      });
      sessionId = result.sessionId;
      runningStats.total++;
      if (correct) runningStats.correct++;

      if (prefetchedQuestion) {
        question = prefetchedQuestion;
        prefetchedQuestion = null;
      } else {
        await fetchQuestion();
      }
      showAnswer = false;
    } catch (err: any) {
      error = err?.message ?? 'Failed to submit answer';
    } finally {
      submitting = false;
    }
  }

  async function handleArchive() {
    if (!question) return;
    try {
      await api.post(`/api/questions/${question.id}/archive`, {
        reason: 'Missing media or problematic question',
      });
      // Move to next question
      if (prefetchedQuestion) {
        question = prefetchedQuestion;
        prefetchedQuestion = null;
      } else {
        await fetchQuestion();
      }
      showAnswer = false;
    } catch (err: any) {
      error = err?.message ?? 'Failed to archive question';
    }
  }

  async function handleEndSession() {
    if (!sessionId) {
      showEndConfirm = false;
      return;
    }
    try {
      const result = await api.post('/api/quiz/complete', { sessionId });
      sessionSummary = result.summary;
      showSessionSummary = true;
    } catch (err: any) {
      error = err?.message ?? 'Failed to end session';
    } finally {
      showEndConfirm = false;
    }
  }

  async function savePreferences() {
    try {
      await api.put('/api/preferences', { gameTypeFilters });
    } catch {
      // Non-critical
    }
  }

  async function handleCategoryChange(value: string) {
    selectedCategory = value;
    prefetchedQuestion = null;
    await fetchQuestion();
  }

  function toggleGameTypeFilter(type: string) {
    if (gameTypeFilters.includes(type)) {
      gameTypeFilters = gameTypeFilters.filter((t) => t !== type);
    } else {
      gameTypeFilters = [...gameTypeFilters, type];
    }
    savePreferences();
    prefetchedQuestion = null;
    fetchQuestion();
  }

  // --- Keyboard shortcuts ---
  function handleKeydown(e: KeyboardEvent) {
    if (e.target instanceof HTMLInputElement || e.target instanceof HTMLSelectElement) return;

    if (e.code === 'Space' && !showAnswer) {
      e.preventDefault();
      showAnswer = true;
      prefetchNextQuestion();
    } else if (e.code === 'ArrowRight' && showAnswer && !submitting) {
      handleAnswer(true);
    } else if (e.code === 'ArrowLeft' && showAnswer && !submitting) {
      handleAnswer(false);
    }
  }

  // --- Mount ---
  onMount(async () => {
    try {
      const [cats, prefs] = await Promise.all([
        api.get('/api/categories'),
        api.get('/api/preferences'),
      ]);
      categories = cats ?? [];
      gameTypeFilters = prefs?.gameTypeFilters ?? [];
    } catch {
      // Non-critical; continue
    }
    await fetchQuestion();
  });
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="min-h-screen bg-gray-50 py-6 px-4">
  <div class="max-w-2xl mx-auto flex flex-col gap-4">

    <!-- Header row -->
    <div class="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-3">
      <h1 class="text-2xl font-bold text-jeopardy-blue">Quiz</h1>

      <!-- Running stats -->
      {#if runningStats.total > 0}
        <div class="text-sm font-medium text-gray-600">
          <span class="text-green-600 font-bold">{runningStats.correct}</span>
          / {runningStats.total} correct
          (<span class="{accuracy >= 75 ? 'text-green-600' : accuracy >= 50 ? 'text-amber-500' : 'text-red-500'} font-bold">{accuracy}%</span>)
        </div>
      {/if}

      <!-- End session button -->
      {#if sessionId}
        <button
          onclick={() => (showEndConfirm = true)}
          class="px-4 py-1.5 rounded-lg border border-gray-300 text-sm text-gray-700 hover:bg-gray-100 transition-colors"
        >
          End Session
        </button>
      {/if}
    </div>

    <!-- Filters -->
    <div class="bg-white rounded-xl shadow-sm px-5 py-4 flex flex-col sm:flex-row sm:items-center gap-4">
      <div class="flex-1">
        <p class="block text-xs font-semibold text-gray-500 uppercase tracking-wide mb-1">Category</p>
        <CategoryFilter
          {categories}
          selected={selectedCategory}
          onchange={handleCategoryChange}
        />
      </div>

      <div>
        <p class="text-xs font-semibold text-gray-500 uppercase tracking-wide mb-2">Exclude Game Types</p>
        <div class="flex flex-wrap gap-3">
          {#each ['Kids', 'Teen', 'College'] as type}
            <label class="flex items-center gap-1.5 text-sm text-gray-700 cursor-pointer">
              <input
                type="checkbox"
                checked={gameTypeFilters.includes(type)}
                onchange={() => toggleGameTypeFilter(type)}
                class="w-4 h-4 rounded border-gray-300 text-jeopardy-blue focus:ring-jeopardy-blue"
              />
              {type}
            </label>
          {/each}
        </div>
      </div>
    </div>

    <!-- Error banner -->
    {#if error}
      <div class="px-4 py-3 bg-red-50 border border-red-200 text-red-700 rounded-lg text-sm">
        {error}
        <button onclick={() => (error = '')} class="ml-2 underline text-red-500">Dismiss</button>
      </div>
    {/if}

    <!-- Question card -->
    {#if loading && !question}
      <div class="flex justify-center py-20">
        <div class="animate-spin rounded-full h-12 w-12 border-b-2 border-jeopardy-blue"></div>
      </div>
    {:else if question}
      <div class="min-h-[420px]">
        <QuestionCard
          clue={question.answer}
          answer={question.question}
          category={question.category}
          classifierCategory={question.classifier_category ?? question.classifierCategory ?? question.category}
          clueValue={question.clue_value ?? question.clueValue ?? null}
          round={question.round ?? null}
          airDate={question.air_date ?? question.airDate ?? null}
          {showAnswer}
          onRevealAnswer={() => {
            showAnswer = true;
            prefetchNextQuestion();
          }}
          onCorrect={() => handleAnswer(true)}
          onIncorrect={() => handleAnswer(false)}
          {submitting}
        >
          {#snippet additionalActions()}
            {#if showAnswer}
              <button
                onclick={handleArchive}
                class="w-full py-2 rounded-lg bg-white/10 hover:bg-white/20 border border-white/20 text-white/80 text-sm font-medium transition-colors"
              >
                Archive (problematic question)
              </button>
            {/if}
          {/snippet}
        </QuestionCard>
      </div>

      <!-- Keyboard hint -->
      <p class="text-center text-xs text-gray-400">
        {#if !showAnswer}
          Press <kbd class="px-1.5 py-0.5 bg-gray-100 rounded border border-gray-300 font-mono">Space</kbd> to reveal answer
        {:else}
          <kbd class="px-1.5 py-0.5 bg-gray-100 rounded border border-gray-300 font-mono">←</kbd> Incorrect &nbsp;
          <kbd class="px-1.5 py-0.5 bg-gray-100 rounded border border-gray-300 font-mono">→</kbd> Correct
        {/if}
      </p>
    {:else}
      <div class="text-center py-16 text-gray-500">No questions available for the selected filters.</div>
    {/if}

  </div>
</div>

<!-- End session confirmation -->
{#if showEndConfirm}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 px-4">
    <div class="w-full max-w-sm rounded-2xl bg-white shadow-2xl p-6 flex flex-col gap-4">
      <h2 class="text-lg font-bold text-gray-800">End Session?</h2>
      <p class="text-sm text-gray-600">This will complete your current quiz session and show a summary.</p>
      <div class="flex gap-3">
        <button
          onclick={() => (showEndConfirm = false)}
          class="flex-1 py-2.5 rounded-xl border border-gray-300 text-gray-700 text-sm font-medium hover:bg-gray-50 transition-colors"
        >
          Cancel
        </button>
        <button
          onclick={handleEndSession}
          class="flex-1 py-2.5 rounded-xl bg-jeopardy-blue text-white text-sm font-semibold hover:bg-blue-800 transition-colors"
        >
          End Session
        </button>
      </div>
    </div>
  </div>
{/if}

<!-- Session summary modal -->
{#if showSessionSummary && sessionSummary}
  <SessionSummary
    summary={sessionSummary}
    onclose={() => {
      showSessionSummary = false;
      sessionId = null;
      runningStats = { total: 0, correct: 0 };
    }}
  />
{/if}
