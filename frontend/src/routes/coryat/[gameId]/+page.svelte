<script lang="ts">
  import { onMount } from 'svelte';
  import { getAuth } from '$lib/auth.svelte';
  import { goto } from '$app/navigation';
  import { api } from '$lib/api';
  import { page } from '$app/state';
  import GameBoard from '$lib/components/GameBoard.svelte';

  const auth = getAuth();

  $effect(() => {
    if (!auth.loading && !auth.user) goto('/login');
  });

  const gameId = page.params.gameId;

  // --- Types ---
  interface BoardQuestion {
    col: number;
    row: number;
    question_id: number | null;
    value: number;
    answered: string | null;
    daily_double: boolean;
  }

  interface Round {
    round: string;
    categories: string[];
    questions: BoardQuestion[];
  }

  interface GameData {
    id: number;
    rounds: Round[];
    final_jeopardy?: { question_id: number | null; category: string };
    completed_at: string | null;
    jeopardy_score: number;
    double_jeopardy_score: number;
    total_score: number;
  }

  // --- State ---
  let game = $state<GameData | null>(null);
  let loading = $state(true);
  let error = $state('');

  // Current round index (0 = Jeopardy, 1 = Double Jeopardy)
  let roundIndex = $state(0);

  // Modal state
  type ModalState = 'none' | 'clue' | 'round_complete' | 'final' | 'game_complete';
  let modalState = $state<ModalState>('none');

  let selectedQuestion = $state<any>(null);
  let selectedCell = $state<{ col: number; row: number } | null>(null);
  let clueLoading = $state(false);
  let showClueAnswer = $state(false);
  let submitting = $state(false);

  // Score tracking (local, synced from server)
  let jeopardyScore = $state(0);
  let doubleJeopardyScore = $state(0);

  // Final jeopardy
  let finalQuestion = $state<any>(null);
  let finalLoading = $state(false);
  let showFinalAnswer = $state(false);

  // Game complete breakdown
  let gameResult = $state<any>(null);
  let completing = $state(false);

  // --- Derived ---
  let currentRound = $derived(game?.rounds[roundIndex] ?? null);
  let currentScore = $derived(
    roundIndex === 0 ? jeopardyScore : jeopardyScore + doubleJeopardyScore
  );
  let roundName = $derived(
    roundIndex === 0 ? 'Jeopardy!' : 'Double Jeopardy!'
  );
  let isRoundComplete = $derived(
    currentRound !== null &&
    currentRound.questions.every((q) => q.question_id === null || q.answered !== null)
  );

  // --- Fetch game ---
  onMount(async () => {
    try {
      game = await api.get(`/api/coryat/${gameId}`);
      if (game) {
        jeopardyScore = game.jeopardy_score ?? 0;
        doubleJeopardyScore = game.double_jeopardy_score ?? 0;

        // If already completed, go to history
        if (game.completed_at) {
          goto('/coryat/history');
          return;
        }

        // Determine which round to show based on answered questions
        const jeopardyRound = game.rounds[0];
        if (jeopardyRound) {
          const allJeopardyDone = jeopardyRound.questions.every(
            (q) => q.question_id === null || q.answered !== null
          );
          if (allJeopardyDone && game.rounds.length > 1) {
            roundIndex = 1;
          }
        }
      }
    } catch (err: any) {
      error = err?.message ?? 'Failed to load game';
    } finally {
      loading = false;
    }
  });

  // --- Cell select ---
  async function handleCellSelect(col: number, row: number) {
    if (clueLoading || submitting) return;
    const q = currentRound?.questions.find((q) => q.col === col && q.row === row);
    if (!q || q.question_id === null || q.answered !== null) return;

    selectedCell = { col, row };
    clueLoading = true;
    showClueAnswer = false;
    modalState = 'clue';

    try {
      selectedQuestion = await api.get(`/api/questions/${q.question_id}`);
    } catch (err: any) {
      error = err?.message ?? 'Failed to load question';
      modalState = 'none';
      selectedCell = null;
    } finally {
      clueLoading = false;
    }
  }

  // --- Answer submission ---
  async function handleAnswer(result: 'correct' | 'incorrect' | 'pass') {
    if (submitting || !selectedCell || !currentRound) return;
    submitting = true;

    const q = currentRound.questions.find(
      (q) => q.col === selectedCell!.col && q.row === selectedCell!.row
    );
    if (!q) { submitting = false; return; }

    try {
      const res = await api.post(`/api/coryat/${gameId}/answer`, {
        question_id: q.question_id,
        col: selectedCell.col,
        row: selectedCell.row,
        round: currentRound.round,
        result,
      });

      // Update local state
      q.answered = result;
      if (roundIndex === 0) {
        jeopardyScore = res.jeopardy_score ?? jeopardyScore;
      } else {
        doubleJeopardyScore = res.double_jeopardy_score ?? doubleJeopardyScore;
      }

      modalState = 'none';
      selectedCell = null;
      selectedQuestion = null;

      // Check if round complete
      if (isRoundComplete) {
        modalState = 'round_complete';
      }
    } catch (err: any) {
      error = err?.message ?? 'Failed to submit answer';
    } finally {
      submitting = false;
    }
  }

  // --- Advance round ---
  function advanceRound() {
    if (roundIndex === 0 && game && game.rounds.length > 1) {
      roundIndex = 1;
      modalState = 'none';
    } else {
      // Move to Final Jeopardy
      modalState = 'none';
      loadFinalJeopardy();
    }
  }

  // --- Final Jeopardy ---
  async function loadFinalJeopardy() {
    if (!game?.final_jeopardy?.question_id) {
      // No final jeopardy question — skip to complete
      await completeGame();
      return;
    }
    finalLoading = true;
    showFinalAnswer = false;
    modalState = 'final';
    try {
      finalQuestion = await api.get(`/api/questions/${game.final_jeopardy.question_id}`);
    } catch (err: any) {
      error = err?.message ?? 'Failed to load Final Jeopardy';
    } finally {
      finalLoading = false;
    }
  }

  // --- Complete game ---
  async function completeGame() {
    completing = true;
    try {
      gameResult = await api.post(`/api/coryat/${gameId}/complete`);
      modalState = 'game_complete';
    } catch (err: any) {
      error = err?.message ?? 'Failed to complete game';
    } finally {
      completing = false;
    }
  }
</script>

<div class="min-h-screen bg-gray-900 py-4 px-2 sm:px-4">
  <div class="max-w-5xl mx-auto flex flex-col gap-4">

    <!-- Header -->
    <div class="flex items-center justify-between">
      <div>
        <h1 class="text-xl sm:text-2xl font-bold text-white">{roundName}</h1>
        <p class="text-yellow-300 text-sm font-semibold mt-0.5">Score: ${currentScore.toLocaleString()}</p>
      </div>
      <a href="/coryat" class="text-gray-400 hover:text-white text-sm transition-colors">Exit</a>
    </div>

    {#if error}
      <div class="px-4 py-3 bg-red-900/60 border border-red-700 text-red-300 rounded-lg text-sm">
        {error}
        <button onclick={() => (error = '')} class="ml-2 underline">Dismiss</button>
      </div>
    {/if}

    {#if loading}
      <div class="flex justify-center py-20">
        <div class="animate-spin rounded-full h-12 w-12 border-b-2 border-yellow-400"></div>
      </div>
    {:else if currentRound}
      <GameBoard
        categories={currentRound.categories}
        questions={currentRound.questions}
        onselect={handleCellSelect}
      />
    {/if}

  </div>
</div>

<!-- Clue Modal -->
{#if modalState === 'clue'}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/80 px-4">
    <div class="w-full max-w-lg bg-jeopardy-blue rounded-2xl shadow-2xl p-6 flex flex-col gap-4">
      {#if clueLoading}
        <div class="flex justify-center py-8">
          <div class="animate-spin rounded-full h-10 w-10 border-b-2 border-yellow-400"></div>
        </div>
      {:else if selectedQuestion}
        <div class="text-center">
          <p class="text-yellow-300 text-xs font-bold uppercase tracking-widest mb-3">
            {selectedQuestion.category ?? ''}
          </p>
          <p class="text-white text-lg sm:text-xl font-semibold leading-relaxed">
            {selectedQuestion.answer}
          </p>
        </div>

        {#if !showClueAnswer}
          <button
            onclick={() => (showClueAnswer = true)}
            class="mt-2 py-3 bg-yellow-400 hover:bg-yellow-300 text-gray-900 font-bold rounded-xl transition-colors"
          >
            Reveal Answer
          </button>
        {:else}
          <div class="bg-white/10 rounded-xl p-4 text-center">
            <p class="text-yellow-200 text-xs font-bold uppercase tracking-widest mb-1">Answer</p>
            <p class="text-white text-lg font-bold">{selectedQuestion.question}</p>
          </div>

          <div class="flex gap-3">
            <button
              onclick={() => handleAnswer('correct')}
              disabled={submitting}
              class="flex-1 py-3 bg-green-500 hover:bg-green-400 disabled:opacity-60 text-white font-bold rounded-xl transition-colors"
            >
              Correct
            </button>
            <button
              onclick={() => handleAnswer('incorrect')}
              disabled={submitting}
              class="flex-1 py-3 bg-red-500 hover:bg-red-400 disabled:opacity-60 text-white font-bold rounded-xl transition-colors"
            >
              Incorrect
            </button>
            <button
              onclick={() => handleAnswer('pass')}
              disabled={submitting}
              class="flex-1 py-3 bg-gray-500 hover:bg-gray-400 disabled:opacity-60 text-white font-bold rounded-xl transition-colors"
            >
              Pass
            </button>
          </div>
        {/if}
      {/if}
    </div>
  </div>
{/if}

<!-- Round Complete Modal -->
{#if modalState === 'round_complete'}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/80 px-4">
    <div class="w-full max-w-sm bg-white rounded-2xl shadow-2xl p-6 flex flex-col gap-4 text-center">
      <h2 class="text-2xl font-bold text-gray-800">{roundName} Complete!</h2>
      {#if roundIndex === 0}
        <p class="text-gray-600">Jeopardy! round score:</p>
        <p class="text-3xl font-bold text-jeopardy-blue">${jeopardyScore.toLocaleString()}</p>
      {:else}
        <p class="text-gray-600">Double Jeopardy! round score:</p>
        <p class="text-3xl font-bold text-jeopardy-blue">${doubleJeopardyScore.toLocaleString()}</p>
        <p class="text-sm text-gray-500">Running total: ${(jeopardyScore + doubleJeopardyScore).toLocaleString()}</p>
      {/if}
      <button
        onclick={advanceRound}
        class="py-3 bg-jeopardy-blue hover:bg-blue-800 text-white font-bold rounded-xl transition-colors"
      >
        Continue
      </button>
    </div>
  </div>
{/if}

<!-- Final Jeopardy Modal -->
{#if modalState === 'final'}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/80 px-4">
    <div class="w-full max-w-lg bg-jeopardy-blue rounded-2xl shadow-2xl p-6 flex flex-col gap-4">
      <h2 class="text-yellow-300 text-center text-xl font-bold tracking-widest uppercase">Final Jeopardy!</h2>
      {#if finalLoading}
        <div class="flex justify-center py-8">
          <div class="animate-spin rounded-full h-10 w-10 border-b-2 border-yellow-400"></div>
        </div>
      {:else if finalQuestion}
        <p class="text-yellow-200 text-center text-xs font-bold uppercase tracking-widest">
          {finalQuestion.category ?? game?.final_jeopardy?.category ?? ''}
        </p>
        <p class="text-white text-lg font-semibold text-center leading-relaxed">
          {finalQuestion.answer}
        </p>

        {#if !showFinalAnswer}
          <button
            onclick={() => (showFinalAnswer = true)}
            class="py-3 bg-yellow-400 hover:bg-yellow-300 text-gray-900 font-bold rounded-xl transition-colors"
          >
            Reveal Answer
          </button>
        {:else}
          <div class="bg-white/10 rounded-xl p-4 text-center">
            <p class="text-yellow-200 text-xs font-bold uppercase tracking-widest mb-1">Answer</p>
            <p class="text-white text-lg font-bold">{finalQuestion.question}</p>
          </div>
          <p class="text-gray-300 text-sm text-center italic">
            Final Jeopardy is not scored in Coryat — it's for viewing only.
          </p>
          <button
            onclick={completeGame}
            disabled={completing}
            class="py-3 bg-green-500 hover:bg-green-400 disabled:opacity-60 text-white font-bold rounded-xl transition-colors"
          >
            {completing ? 'Finishing...' : 'Finish Game'}
          </button>
        {/if}
      {/if}
    </div>
  </div>
{/if}

<!-- Game Complete Modal -->
{#if modalState === 'game_complete' && gameResult}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/80 px-4">
    <div class="w-full max-w-sm bg-white rounded-2xl shadow-2xl p-6 flex flex-col gap-4 text-center">
      <h2 class="text-2xl font-bold text-gray-800">Game Complete!</h2>

      <div class="flex flex-col gap-2">
        <div class="flex justify-between py-2 border-b border-gray-100">
          <span class="text-gray-600">Jeopardy!</span>
          <span class="font-semibold text-gray-800">${(gameResult.jeopardy_score ?? 0).toLocaleString()}</span>
        </div>
        <div class="flex justify-between py-2 border-b border-gray-100">
          <span class="text-gray-600">Double Jeopardy!</span>
          <span class="font-semibold text-gray-800">${(gameResult.double_jeopardy_score ?? 0).toLocaleString()}</span>
        </div>
        <div class="flex justify-between py-2">
          <span class="font-bold text-gray-800">Coryat Score</span>
          <span class="font-bold text-2xl text-jeopardy-blue">${(gameResult.total_score ?? 0).toLocaleString()}</span>
        </div>
      </div>

      <div class="flex gap-3 mt-2">
        <a
          href="/coryat/history"
          class="flex-1 py-2.5 border border-gray-300 text-gray-700 font-semibold rounded-xl hover:bg-gray-50 transition-colors"
        >
          View History
        </a>
        <a
          href="/coryat"
          class="flex-1 py-2.5 bg-jeopardy-blue text-white font-semibold rounded-xl hover:bg-blue-800 transition-colors"
        >
          Play Again
        </a>
      </div>
    </div>
  </div>
{/if}
