<script lang="ts">
  import { onMount } from 'svelte';
  import { getAuth } from '$lib/auth.svelte';
  import { goto } from '$app/navigation';
  import { api } from '$lib/api';

  const auth = getAuth();
  $effect(() => {
    if (!auth.loading && !auth.user) goto('/login');
  });

  interface Pack {
    id: number;
    theme: string;
    diagnosis: string;
    primer: string;
    searchQuery: string;
    matchCount: number;
  }

  let packs = $state<Pack[]>([]);
  let generatedAt = $state<string | null>(null);
  let insufficientData = $state(false);
  let configured = $state(true);
  let loading = $state(true);
  let refreshing = $state(false);
  let error = $state('');

  function apply(res: any) {
    packs = res.packs ?? [];
    generatedAt = res.generatedAt ?? null;
    insufficientData = res.insufficientData ?? false;
    configured = res.configured ?? true;
  }

  onMount(async () => {
    try {
      apply(await api.get('/api/blindspots'));
    } catch (err: any) {
      error = err?.message ?? 'Failed to load blind spots';
    } finally {
      loading = false;
    }
  });

  async function refresh() {
    refreshing = true;
    error = '';
    try {
      apply(await api.post('/api/blindspots/generate'));
    } catch (err: any) {
      error = err?.message ?? 'Failed to analyze blind spots';
    } finally {
      refreshing = false;
    }
  }
</script>

<svelte:head>
  <title>Blind Spots — Jeopardy! Training</title>
</svelte:head>

<div class="min-h-screen bg-gray-50 py-6 px-4">
  <div class="max-w-2xl mx-auto flex flex-col gap-4">
    <div class="flex items-center gap-2 flex-wrap">
      <h1 class="text-2xl font-bold text-jeopardy-blue">Blind Spots</h1>
      {#if generatedAt}
        <span class="text-sm text-gray-500">as of {new Date(generatedAt).toLocaleDateString()}</span>
      {/if}
      <div class="flex items-center gap-2 ml-auto">
        {#if configured}
          <button
            onclick={refresh}
            disabled={refreshing}
            class="px-3 py-1.5 rounded-lg border border-gray-300 text-sm text-gray-700 hover:bg-gray-100 disabled:opacity-50 transition-colors"
          >
            {refreshing ? 'Analyzing…' : 'Refresh'}
          </button>
        {/if}
        <button
          onclick={() => goto('/dashboard')}
          class="px-3 py-1.5 rounded-lg border border-gray-300 text-sm text-gray-700 hover:bg-gray-100 transition-colors"
        >
          Done
        </button>
      </div>
    </div>

    {#if error}
      <div class="px-4 py-3 bg-red-50 border border-red-200 text-red-700 rounded-lg text-sm">
        {error}
        <button onclick={() => (error = '')} class="ml-2 underline text-red-500">Dismiss</button>
      </div>
    {/if}

    {#if loading}
      <div class="flex justify-center py-16">
        <div class="animate-spin rounded-full h-10 w-10 border-b-2 border-jeopardy-blue"></div>
      </div>
    {:else if !configured}
      <div class="text-center py-16 text-gray-500">
        Blind-spot analysis is not configured (missing OpenAI key).
      </div>
    {:else if packs.length === 0}
      <div class="text-center py-16 text-gray-500">
        {#if insufficientData}
          Not enough misses to analyze yet — keep practicing and check back.
        {:else}
          No blind spots analyzed yet. Hit Refresh to run the first analysis.
        {/if}
      </div>
    {:else}
      <div class="flex flex-col gap-3">
        {#each packs as pack (pack.id)}
          <div class="bg-white rounded-xl shadow-sm p-5 flex flex-col gap-2">
            <div class="flex items-center gap-2 flex-wrap">
              <h2 class="text-lg font-bold text-gray-800">{pack.theme}</h2>
              <a
                href={`/drill?q=${encodeURIComponent(pack.searchQuery)}`}
                class="ml-auto px-4 py-1.5 rounded-lg bg-jeopardy-blue text-white text-sm font-semibold hover:bg-blue-800 transition-colors"
              >
                Drill this ({pack.matchCount} clues)
              </a>
            </div>
            <p class="text-sm text-red-600">{pack.diagnosis}</p>
            <p class="text-sm text-gray-700 leading-relaxed">{pack.primer}</p>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>
