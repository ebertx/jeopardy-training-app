<script lang="ts">
  import { onMount } from 'svelte';
  import { getAuth } from '$lib/auth.svelte';
  import { goto } from '$app/navigation';
  import { api } from '$lib/api';
  import QuestionCard from '$lib/components/QuestionCard.svelte';
  import CategoryFilter from '$lib/components/CategoryFilter.svelte';
  import MasteryBadge from '$lib/components/MasteryBadge.svelte';

  const auth = getAuth();

  $effect(() => {
    if (!auth.loading && !auth.user) goto('/login');
  });

  // --- Types ---
  interface ReviewItem {
    question: {
      id: number;
      question: string;
      answer: string;
      category: string;
      classifier_category: string;
      clue_value: number | null;
      round: number | null;
      air_date: string | null;
    };
    masteryProgress: {
      consecutive_correct: number;
      required: number;
    };
  }

  // --- State ---
  let reviewItems = $state<ReviewItem[]>([]);
  let categories = $state<Array<{ name: string; count: number }>>([]);
  let selectedCategory = $state('all');
  let loading = $state(true);
  let error = $state('');
  let expandedId = $state<number | null>(null);
  let archivingId = $state<number | null>(null);

  // Review session state
  let inSession = $state(false);
  let sessionItems = $state<ReviewItem[]>([]);
  let sessionIndex = $state(0);
  let showAnswer = $state(false);
  let submitting = $state(false);
  let sessionId = $state<number | null>(null);

  // Sorted: closest to mastery first (highest consecutive_correct)
  let sortedItems = $derived(
    [...reviewItems].sort(
      (a, b) => b.masteryProgress.consecutive_correct - a.masteryProgress.consecutive_correct
    )
  );

  let currentSessionItem = $derived(
    sessionItems.length > 0 && sessionIndex < sessionItems.length
      ? sessionItems[sessionIndex]
      : null
  );

  // --- API ---
  async function fetchReviewItems() {
    loading = true;
    error = '';
    try {
      const params = new URLSearchParams();
      if (selectedCategory !== 'all') params.set('category', selectedCategory);
      reviewItems = await api.get(`/api/review?${params}`);
    } catch (err: any) {
      error = err?.message ?? 'Failed to load review items';
    } finally {
      loading = false;
    }
  }

  async function handleArchive(questionId: number) {
    archivingId = questionId;
    try {
      await api.post(`/api/questions/${questionId}/archive`, {
        reason: 'Archived from review',
      });
      reviewItems = reviewItems.filter((item) => item.question.id !== questionId);
      expandedId = null;
    } catch (err: any) {
      error = err?.message ?? 'Failed to archive question';
    } finally {
      archivingId = null;
    }
  }

  async function handleCategoryChange(value: string) {
    selectedCategory = value;
    await fetchReviewItems();
  }

  // --- Review Session ---
  function startSession() {
    sessionItems = [...sortedItems];
    sessionIndex = 0;
    showAnswer = false;
    sessionId = null;
    inSession = true;
  }

  async function handleSessionAnswer(correct: boolean) {
    if (!currentSessionItem || submitting) return;
    submitting = true;
    try {
      const result = await api.post('/api/quiz/submit', {
        questionId: currentSessionItem.question.id,
        correct,
        sessionId,
        isReviewSession: true,
      });
      sessionId = result.sessionId;
      showAnswer = false;

      if (sessionIndex + 1 >= sessionItems.length) {
        // Session complete
        await fetchReviewItems();
        inSession = false;
        sessionItems = [];
        sessionIndex = 0;
        sessionId = null;
      } else {
        sessionIndex++;
      }
    } catch (err: any) {
      error = err?.message ?? 'Failed to submit answer';
    } finally {
      submitting = false;
    }
  }

  function endSession() {
    inSession = false;
    sessionItems = [];
    sessionIndex = 0;
    showAnswer = false;
    sessionId = null;
  }

  // --- Keyboard shortcuts for session ---
  function handleKeydown(e: KeyboardEvent) {
    if (!inSession) return;
    if (e.target instanceof HTMLInputElement || e.target instanceof HTMLSelectElement) return;

    if (e.code === 'Space' && !showAnswer) {
      e.preventDefault();
      showAnswer = true;
    } else if (e.code === 'ArrowRight' && showAnswer && !submitting) {
      handleSessionAnswer(true);
    } else if (e.code === 'ArrowLeft' && showAnswer && !submitting) {
      handleSessionAnswer(false);
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
    await fetchReviewItems();
  });
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="min-h-screen bg-gray-50 py-6 px-4">
  <div class="max-w-2xl mx-auto flex flex-col gap-4">

    {#if inSession}
      <!-- ===== REVIEW SESSION ===== -->
      <div class="flex items-center justify-between">
        <h1 class="text-2xl font-bold text-jeopardy-blue">Review Session</h1>
        <button
          onclick={endSession}
          class="px-4 py-1.5 rounded-lg border border-gray-300 text-sm text-gray-700 hover:bg-gray-100 transition-colors"
        >
          End Session
        </button>
      </div>

      <!-- Progress bar -->
      <div>
        <div class="flex justify-between text-xs text-gray-500 mb-1">
          <span>Question {sessionIndex + 1} of {sessionItems.length}</span>
          <span>{Math.round(((sessionIndex) / sessionItems.length) * 100)}%</span>
        </div>
        <div class="w-full bg-gray-200 rounded-full h-2">
          <div
            class="bg-jeopardy-blue h-2 rounded-full transition-all"
            style="width: {(sessionIndex / sessionItems.length) * 100}%"
          ></div>
        </div>
      </div>

      {#if error}
        <div class="px-4 py-3 bg-red-50 border border-red-200 text-red-700 rounded-lg text-sm">
          {error}
          <button onclick={() => (error = '')} class="ml-2 underline text-red-500">Dismiss</button>
        </div>
      {/if}

      {#if currentSessionItem}
        <div class="min-h-[420px]">
          <QuestionCard
            clue={currentSessionItem.question.question}
            answer={currentSessionItem.question.answer}
            category={currentSessionItem.question.category}
            classifierCategory={currentSessionItem.question.classifier_category}
            clueValue={currentSessionItem.question.clue_value}
            round={currentSessionItem.question.round}
            airDate={currentSessionItem.question.air_date}
            {showAnswer}
            onRevealAnswer={() => { showAnswer = true; }}
            onCorrect={() => handleSessionAnswer(true)}
            onIncorrect={() => handleSessionAnswer(false)}
            {submitting}
          >
            {#snippet badge()}
              <MasteryBadge
                consecutiveCorrect={currentSessionItem.masteryProgress.consecutive_correct}
                required={currentSessionItem.masteryProgress.required}
              />
            {/snippet}
          </QuestionCard>
        </div>

        <p class="text-center text-xs text-gray-400">
          {#if !showAnswer}
            Press <kbd class="px-1.5 py-0.5 bg-gray-100 rounded border border-gray-300 font-mono">Space</kbd> to reveal answer
          {:else}
            <kbd class="px-1.5 py-0.5 bg-gray-100 rounded border border-gray-300 font-mono">←</kbd> Incorrect &nbsp;
            <kbd class="px-1.5 py-0.5 bg-gray-100 rounded border border-gray-300 font-mono">→</kbd> Correct
          {/if}
        </p>
      {/if}

    {:else}
      <!-- ===== LIST VIEW ===== -->
      <div class="flex items-center justify-between">
        <h1 class="text-2xl font-bold text-jeopardy-blue">Review</h1>
        {#if reviewItems.length > 0}
          <button
            onclick={startSession}
            class="px-4 py-2 bg-jeopardy-blue text-white text-sm font-semibold rounded-lg hover:bg-blue-800 transition-colors"
          >
            Start Review Session
          </button>
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
          <div class="animate-spin rounded-full h-12 w-12 border-b-2 border-jeopardy-blue"></div>
        </div>
      {:else if sortedItems.length === 0}
        <div class="text-center py-16 text-gray-500">
          <p class="text-lg font-medium">No questions to review!</p>
          <p class="text-sm mt-1">Answer some questions incorrectly to see them here.</p>
        </div>
      {:else}
        <div class="flex flex-col gap-3">
          {#each sortedItems as item (item.question.id)}
            {@const expanded = expandedId === item.question.id}
            <div class="bg-white rounded-xl shadow-sm overflow-hidden border border-gray-100">
              <!-- Card header (always visible) -->
              <button
                class="w-full text-left px-5 py-4 hover:bg-gray-50 transition-colors"
                onclick={() => { expandedId = expanded ? null : item.question.id; }}
              >
                <div class="flex items-center gap-2 flex-wrap mb-2">
                  <span class="inline-flex items-center px-2 py-0.5 rounded-full text-xs font-semibold bg-blue-100 text-blue-800">
                    {item.question.classifier_category}
                  </span>
                  {#if item.question.clue_value}
                    <span class="text-xs text-gray-500 font-medium">${item.question.clue_value.toLocaleString()}</span>
                  {/if}
                  <MasteryBadge
                    consecutiveCorrect={item.masteryProgress.consecutive_correct}
                    required={item.masteryProgress.required}
                  />
                  <span class="ml-auto text-gray-400 text-xs">{expanded ? '▲' : '▼'}</span>
                </div>
                <p class="text-sm text-gray-700 line-clamp-2">{item.question.question}</p>
              </button>

              <!-- Expanded content -->
              {#if expanded}
                <div class="px-5 pb-4 border-t border-gray-100 pt-3">
                  <div class="mb-3">
                    <p class="text-xs font-semibold text-gray-500 uppercase tracking-wide mb-1">Correct Response</p>
                    <p class="text-gray-900 font-medium">{item.question.answer}</p>
                  </div>
                  <div class="flex flex-wrap gap-4 text-xs text-gray-500 mb-4">
                    <span>
                      <span class="font-semibold">Category:</span> {item.question.category}
                    </span>
                    {#if item.question.air_date}
                      <span>
                        <span class="font-semibold">Air Date:</span> {item.question.air_date}
                      </span>
                    {/if}
                  </div>
                  <button
                    onclick={() => handleArchive(item.question.id)}
                    disabled={archivingId === item.question.id}
                    class="px-3 py-1.5 text-xs rounded-lg border border-gray-300 text-gray-600 hover:bg-gray-50 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
                  >
                    {archivingId === item.question.id ? 'Archiving...' : 'Archive Question'}
                  </button>
                </div>
              {/if}
            </div>
          {/each}
        </div>
      {/if}
    {/if}

  </div>
</div>
