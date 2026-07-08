<script lang="ts">
  import { page } from '$app/state';
  import { getAuth } from '$lib/auth.svelte';
  import { goto } from '$app/navigation';
  import { api } from '$lib/api';
  import { marked } from 'marked';
  import DOMPurify from 'dompurify';

  const auth = getAuth();
  $effect(() => {
    if (!auth.loading && !auth.user) goto('/login');
  });

  let primer = $state<{ topic: string; contentMd: string; createdAt: string } | null>(null);
  let error = $state('');
  let html = $state('');

  $effect(() => {
    const slug = page.params.slug;
    error = '';
    primer = null;
    html = '';
    let stale = false;
    api.get(`/api/primers/${slug}`)
      .then(async (p) => {
        if (stale) return;
        const rendered = DOMPurify.sanitize(await marked.parse(p.contentMd));
        if (stale) return;
        primer = p;
        html = rendered;
      })
      .catch((e) => {
        if (!stale) error = e?.message ?? 'Not found';
      });
    return () => {
      stale = true;
    };
  });
</script>

<svelte:head><title>{primer?.topic ?? 'Primer'} — Jeopardy! Training</title></svelte:head>

<div class="min-h-screen bg-gray-50 py-8 px-4">
  <div class="max-w-3xl mx-auto">
    <div class="mb-4 flex items-center justify-between">
      <a href="/primers" class="text-sm text-jeopardy-blue hover:underline">&larr; All primers</a>
      {#if primer}
        <a href="/drill?q={encodeURIComponent(primer.topic)}"
          class="px-4 py-2 rounded-lg bg-jeopardy-blue text-white text-sm font-semibold hover:bg-blue-800">
          Drill this topic &rarr;
        </a>
      {/if}
    </div>
    {#if error}
      <div class="px-4 py-3 bg-red-50 border border-red-200 text-red-700 rounded-lg">{error}</div>
    {:else if primer}
      <article class="primer-content bg-white rounded-xl shadow p-8">
        <h1>{primer.topic}</h1>
        {@html html}
      </article>
    {:else}
      <div class="flex justify-center py-16"><div class="animate-spin rounded-full h-12 w-12 border-b-2 border-jeopardy-blue"></div></div>
    {/if}
  </div>
</div>

<style>
  .primer-content {
    color: #1f2937; /* text-gray-800 */
    line-height: 1.75;
  }

  .primer-content h1 {
    font-size: 1.875rem;
    font-weight: 700;
    color: var(--color-jeopardy-blue);
    margin-bottom: 1rem;
  }

  /* h2/h3/p/etc. come from {@html} (the rendered markdown), so scoped CSS
     can't reach them without :global() — h1 is authored directly and needs none. */
  .primer-content :global(h2) {
    font-size: 1.375rem;
    font-weight: 700;
    color: var(--color-jeopardy-blue);
    margin-top: 2rem;
    margin-bottom: 0.75rem;
  }

  .primer-content :global(h3) {
    font-size: 1.125rem;
    font-weight: 600;
    color: var(--color-jeopardy-blue);
    margin-top: 1.5rem;
    margin-bottom: 0.5rem;
  }

  .primer-content :global(p) {
    margin-bottom: 1rem;
  }

  .primer-content :global(strong) {
    font-weight: 700;
    color: #111827; /* text-gray-900 */
  }

  .primer-content :global(ul),
  .primer-content :global(ol) {
    margin-bottom: 1rem;
    padding-left: 1.5rem;
  }

  .primer-content :global(ul) {
    list-style-type: disc;
  }

  .primer-content :global(ol) {
    list-style-type: decimal;
  }

  .primer-content :global(li) {
    margin-bottom: 0.375rem;
  }

  .primer-content :global(table) {
    width: 100%;
    border-collapse: collapse;
    margin-bottom: 1.5rem;
    font-size: 0.875rem;
  }

  .primer-content :global(th),
  .primer-content :global(td) {
    border: 1px solid #e5e7eb; /* border-gray-200 */
    padding: 0.5rem 0.75rem;
    text-align: left;
  }

  .primer-content :global(th) {
    background-color: #f9fafb; /* bg-gray-50 */
    font-weight: 600;
  }

  .primer-content :global(a) {
    color: var(--color-jeopardy-blue);
    text-decoration: underline;
  }

  .primer-content :global(blockquote) {
    border-left: 3px solid #d1d5db;
    padding-left: 1rem;
    color: #6b7280;
    margin-bottom: 1rem;
  }

  .primer-content :global(code) {
    background-color: #f3f4f6;
    padding: 0.125rem 0.375rem;
    border-radius: 0.25rem;
    font-size: 0.875em;
  }
</style>
