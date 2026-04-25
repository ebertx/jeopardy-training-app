<script lang="ts">
  import { onMount } from 'svelte';
  import { getAuth } from '$lib/auth.svelte';
  import { goto } from '$app/navigation';
  import { api } from '$lib/api';

  const auth = getAuth();

  $effect(() => {
    if (!auth.loading && !auth.user) goto('/login');
  });

  interface GameRecord {
    id: number;
    completed_at: string | null;
    jeopardy_score: number;
    double_jeopardy_score: number;
    total_score: number;
    created_at: string;
  }

  let history = $state<GameRecord[]>([]);
  let loading = $state(true);
  let starting = $state(false);
  let error = $state('');

  let totalGames = $derived(history.filter((g) => g.completed_at !== null).length);
  let avgScore = $derived(
    totalGames > 0
      ? Math.round(
          history.filter((g) => g.completed_at !== null).reduce((sum, g) => sum + g.total_score, 0) /
            totalGames
        )
      : 0
  );
  let bestScore = $derived(
    totalGames > 0
      ? Math.max(...history.filter((g) => g.completed_at !== null).map((g) => g.total_score))
      : 0
  );
  let incompleteGame = $derived(history.find((g) => g.completed_at === null) ?? null);

  onMount(async () => {
    try {
      history = await api.get('/api/coryat/history');
    } catch (err: any) {
      error = err?.message ?? 'Failed to load history';
    } finally {
      loading = false;
    }
  });

  async function startGame() {
    starting = true;
    error = '';
    try {
      const result = await api.post('/api/coryat');
      goto(`/coryat/${result.game_id}`);
    } catch (err: any) {
      error = err?.message ?? 'Failed to start game';
      starting = false;
    }
  }
</script>

<div class="min-h-screen bg-gray-50 py-8 px-4">
  <div class="max-w-3xl mx-auto flex flex-col gap-6">

    <!-- Header -->
    <div>
      <h1 class="text-3xl font-bold text-jeopardy-blue mb-2">Coryat Score Practice</h1>
      <p class="text-gray-600">
        The Coryat score is a self-scoring method where you play along with Jeopardy! without wagering.
        Correct answers add the clue value; incorrect answers subtract the clue value. Passes are free.
        This gives you a consistent benchmark independent of luck.
      </p>
    </div>

    <!-- Benchmark Table -->
    <div class="bg-white rounded-xl shadow p-6">
      <h2 class="text-lg font-semibold text-gray-800 mb-4">Score Benchmarks</h2>
      <table class="min-w-full text-sm">
        <thead>
          <tr class="border-b border-gray-200">
            <th class="text-left py-2 px-3 font-semibold text-gray-600">Score Range</th>
            <th class="text-left py-2 px-3 font-semibold text-gray-600">Level</th>
          </tr>
        </thead>
        <tbody>
          <tr class="border-b border-gray-100">
            <td class="py-2 px-3 text-red-600 font-medium">Below $16,000</td>
            <td class="py-2 px-3 text-gray-700">Keep practicing</td>
          </tr>
          <tr class="border-b border-gray-100">
            <td class="py-2 px-3 text-amber-600 font-medium">~$24,000</td>
            <td class="py-2 px-3 text-gray-700">Average contestant</td>
          </tr>
          <tr class="border-b border-gray-100">
            <td class="py-2 px-3 text-blue-600 font-medium">~$28,000</td>
            <td class="py-2 px-3 text-gray-700">Strong player</td>
          </tr>
          <tr>
            <td class="py-2 px-3 text-green-600 font-medium">$32,000+</td>
            <td class="py-2 px-3 text-gray-700">Excellent / champion level</td>
          </tr>
        </tbody>
      </table>
    </div>

    <!-- User Stats -->
    {#if loading}
      <div class="flex justify-center py-8">
        <div class="animate-spin rounded-full h-10 w-10 border-b-2 border-jeopardy-blue"></div>
      </div>
    {:else if totalGames > 0}
      <div class="bg-white rounded-xl shadow p-6">
        <h2 class="text-lg font-semibold text-gray-800 mb-4">Your Stats</h2>
        <div class="flex flex-wrap gap-4">
          <div class="flex-1 min-w-[120px] bg-gray-50 rounded-lg p-4 text-center">
            <p class="text-sm text-gray-500 mb-1">Games Played</p>
            <p class="text-2xl font-bold text-jeopardy-blue">{totalGames}</p>
          </div>
          <div class="flex-1 min-w-[120px] bg-gray-50 rounded-lg p-4 text-center">
            <p class="text-sm text-gray-500 mb-1">Avg Score</p>
            <p class="text-2xl font-bold text-jeopardy-blue">${avgScore.toLocaleString()}</p>
          </div>
          <div class="flex-1 min-w-[120px] bg-gray-50 rounded-lg p-4 text-center">
            <p class="text-sm text-gray-500 mb-1">Best Score</p>
            <p class="text-2xl font-bold text-green-600">${bestScore.toLocaleString()}</p>
          </div>
        </div>
      </div>
    {/if}

    <!-- Error -->
    {#if error}
      <div class="px-4 py-3 bg-red-50 border border-red-200 text-red-700 rounded-lg text-sm">
        {error}
      </div>
    {/if}

    <!-- Actions -->
    <div class="flex flex-wrap gap-3">
      {#if incompleteGame}
        <a
          href="/coryat/{incompleteGame.id}"
          class="px-6 py-3 bg-amber-500 hover:bg-amber-600 text-white font-semibold rounded-lg transition-colors"
        >
          Resume Game
        </a>
      {/if}
      <button
        onclick={startGame}
        disabled={starting}
        class="px-6 py-3 bg-jeopardy-blue hover:bg-blue-800 disabled:opacity-60 text-white font-semibold rounded-lg transition-colors"
      >
        {starting ? 'Starting...' : 'Start New Game'}
      </button>
      <a
        href="/coryat/history"
        class="px-6 py-3 border border-gray-300 hover:bg-gray-100 text-gray-700 font-semibold rounded-lg transition-colors"
      >
        View History
      </a>
    </div>

  </div>
</div>
