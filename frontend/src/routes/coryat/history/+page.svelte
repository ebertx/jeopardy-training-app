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
    final_score: number;
    started_at: string;
  }

  let history = $state<GameRecord[]>([]);
  let loading = $state(true);
  let error = $state('');

  let completedGames = $derived(
    [...history.filter((g) => g.completed_at !== null)].sort(
      (a, b) => new Date(b.completed_at!).getTime() - new Date(a.completed_at!).getTime()
    )
  );

  function formatDate(dateStr: string): string {
    return new Date(dateStr).toLocaleDateString('en-US', {
      year: 'numeric',
      month: 'short',
      day: 'numeric',
    });
  }

  onMount(async () => {
    try {
      history = await api.get('/api/coryat/history');
    } catch (err: any) {
      error = err?.message ?? 'Failed to load history';
    } finally {
      loading = false;
    }
  });
</script>

<div class="min-h-screen bg-gray-50 py-8 px-4">
  <div class="max-w-3xl mx-auto flex flex-col gap-6">

    <div class="flex items-center gap-4">
      <a href="/coryat" class="text-jeopardy-blue hover:underline text-sm">&larr; Back to Coryat</a>
      <h1 class="text-2xl font-bold text-jeopardy-blue">Game History</h1>
    </div>

    {#if error}
      <div class="px-4 py-3 bg-red-50 border border-red-200 text-red-700 rounded-lg text-sm">
        {error}
      </div>
    {/if}

    {#if loading}
      <div class="flex justify-center py-16">
        <div class="animate-spin rounded-full h-10 w-10 border-b-2 border-jeopardy-blue"></div>
      </div>
    {:else if completedGames.length === 0}
      <div class="bg-white rounded-xl shadow p-8 text-center text-gray-500">
        No completed games yet. <a href="/coryat" class="text-jeopardy-blue hover:underline">Start a game</a>!
      </div>
    {:else}
      <div class="bg-white rounded-xl shadow overflow-hidden">
        <div class="overflow-x-auto">
          <table class="min-w-full text-sm">
            <thead class="bg-jeopardy-blue text-white">
              <tr>
                <th class="text-left py-3 px-4 font-semibold">Date</th>
                <th class="text-right py-3 px-4 font-semibold">Jeopardy!</th>
                <th class="text-right py-3 px-4 font-semibold">Double Jeopardy!</th>
                <th class="text-right py-3 px-4 font-semibold">Coryat Score</th>
              </tr>
            </thead>
            <tbody>
              {#each completedGames as game, i}
                <tr class="border-b border-gray-100 {i % 2 === 0 ? '' : 'bg-gray-50'} hover:bg-blue-50 transition-colors">
                  <td class="py-3 px-4 text-gray-700">{formatDate(game.completed_at!)}</td>
                  <td class="py-3 px-4 text-right text-gray-700">${game.jeopardy_score.toLocaleString()}</td>
                  <td class="py-3 px-4 text-right text-gray-700">${game.double_jeopardy_score.toLocaleString()}</td>
                  <td class="py-3 px-4 text-right font-bold {game.final_score >= 32000 ? 'text-green-600' : game.final_score >= 28000 ? 'text-blue-600' : game.final_score >= 24000 ? 'text-amber-600' : 'text-red-600'}">
                    ${game.final_score.toLocaleString()}
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      </div>
    {/if}

  </div>
</div>
