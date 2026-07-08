<script lang="ts">
  import { getAuth } from '$lib/auth.svelte';
  import { goto } from '$app/navigation';
  import { api } from '$lib/api';
  import { onMount } from 'svelte';

  const auth = getAuth();
  $effect(() => {
    if (!auth.loading && !auth.user) goto('/login');
  });

  let primers = $state<Array<{ id: number; slug: string; topic: string; source: string; createdAt: string }>>([]);
  let canon = $state<string[]>([]);
  let configured = $state(true);
  let loading = $state(true);
  let error = $state('');
  let generating = $state<string | null>(null); // topic currently generating
  let customTopic = $state('');

  onMount(async () => {
    try {
      const res = await api.get('/api/primers');
      primers = res.primers;
      canon = res.canon;
      configured = res.configured;
    } catch (e: any) {
      error = e?.message ?? 'Failed to load primers';
    } finally {
      loading = false;
    }
  });

  let existingTopics = $derived(new Set(primers.map((p) => p.topic)));

  async function generate(topic: string, source?: string) {
    if (generating) return;
    generating = topic;
    error = '';
    try {
      const res = await api.post('/api/primers/generate', { topic, source });
      goto(`/primers/${res.slug}`);
    } catch (e: any) {
      error = e?.message ?? 'Generation failed';
    } finally {
      generating = null;
    }
  }
</script>

<svelte:head><title>Primers — Jeopardy! Training</title></svelte:head>

<div class="min-h-screen bg-gray-50 py-8 px-4">
  <div class="max-w-4xl mx-auto">
    <h1 class="text-3xl font-bold text-jeopardy-blue mb-2">Primers</h1>
    <p class="text-gray-500 mb-6">Long-form study guides for the canonical Jeopardy topics. Read the primer, then drill the topic.</p>

    {#if error}<div class="px-4 py-3 mb-4 bg-red-50 border border-red-200 text-red-700 rounded-lg">{error}</div>{/if}
    {#if !configured}<div class="px-4 py-3 mb-4 bg-amber-50 border border-amber-200 text-amber-700 rounded-lg">Generation is not configured (no API key) — existing primers are still readable.</div>{/if}

    {#if loading}
      <div class="flex justify-center py-16"><div class="animate-spin rounded-full h-12 w-12 border-b-2 border-jeopardy-blue"></div></div>
    {:else}
      {#if primers.length > 0}
        <div class="bg-white rounded-xl shadow divide-y divide-gray-100 mb-8">
          {#each primers as p}
            <a href="/primers/{p.slug}" class="flex items-center justify-between p-4 hover:bg-gray-50 transition-colors group">
              <div>
                <p class="font-semibold text-gray-800">{p.topic}</p>
                <p class="text-xs text-gray-400">{p.source} · {new Date(p.createdAt).toLocaleDateString()}</p>
              </div>
              <span class="text-gray-400 group-hover:text-gray-600">&rarr;</span>
            </a>
          {/each}
        </div>
      {/if}

      <h2 class="text-sm font-semibold text-gray-600 mb-3">Generate a primer</h2>
      <div class="flex flex-wrap gap-2 mb-4">
        {#each canon.filter((t) => !existingTopics.has(t)) as topic}
          <button
            onclick={() => generate(topic, 'canon')}
            disabled={generating !== null || !configured}
            class="px-3 py-1.5 rounded-full border border-jeopardy-blue text-jeopardy-blue text-sm font-medium hover:bg-jeopardy-blue hover:text-white transition-colors disabled:opacity-50"
          >
            {generating === topic ? 'Generating… (~30s)' : `+ ${topic}`}
          </button>
        {/each}
      </div>
      <form
        class="flex gap-2"
        onsubmit={(e) => { e.preventDefault(); if (customTopic.trim()) generate(customTopic.trim()); }}
      >
        <input
          bind:value={customTopic}
          placeholder="Custom topic (e.g. 'French Revolution')"
          disabled={generating !== null || !configured}
          class="flex-1 px-4 py-2 border border-gray-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-jeopardy-blue"
        />
        <button type="submit" disabled={generating !== null || !configured || !customTopic.trim()}
          class="px-4 py-2 bg-jeopardy-blue text-white font-semibold rounded-lg hover:bg-blue-800 disabled:opacity-50">
          {generating === customTopic.trim() ? 'Generating…' : 'Generate'}
        </button>
      </form>
    {/if}
  </div>
</div>
