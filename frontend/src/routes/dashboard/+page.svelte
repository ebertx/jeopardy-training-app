<script lang="ts">
  import { getAuth } from '$lib/auth.svelte';
  import { goto } from '$app/navigation';
  import { api } from '$lib/api';
  import { onMount } from 'svelte';
  import StatsSection from '$lib/components/StatsSection.svelte';
  import PavlovProgressCard from '$lib/components/PavlovProgressCard.svelte';
  import type { CategoryStat, DailyStat, DeckStats, ForecastDay, KindStat, PavlovStats } from '$lib/stats';

  interface Stats {
    overall: KindStat;
    cold: KindStat;
    review: KindStat;
    cold30d: KindStat;
    mockReadiness: {
      tests: Array<{ id: number; completedAt: string; score: number }>;
      best: number | null;
      latest: number | null;
      passLine: number;
    };
    projectedMock: {
      score: number;
      passLine: number;
      categories: Array<{
        category: string;
        share: number;
        coldAccuracy: number;
        contribution: number;
        headroom: number;
        estimated: boolean;
      }>;
    };
    categoryBreakdown: CategoryStat[];
    dailyAccuracy: DailyStat[];
  }

  interface SrsStatus {
    dueCount: number;
    newRemaining: number;
    reviewedToday: number;
    forecast: ForecastDay[];
    adaptiveWeights?: Array<{ category: string; attempts: number; accuracy: number; weight: number }>;
    adaptiveWindow?: '180d' | 'all' | null;
    deck?: DeckStats;
  }

  const auth = getAuth();

  $effect(() => {
    if (!auth.loading && !auth.user) goto('/login');
  });

  let stats = $state<Stats | null>(null);
  let srs = $state<SrsStatus | null>(null);
  let pavlov = $state<PavlovStats | null>(null);
  let loading = $state(true);
  let error = $state('');

  // Phone layout below Tailwind's `sm` breakpoint. SSR sees 0 → desktop
  // config; corrected on hydration (charts are client-only anyway).
  let innerWidth = $state(0);
  let isMobile = $derived(innerWidth > 0 && innerWidth < 640);

  let blindspots = $state<{
    packs: Array<{ id: number; theme: string; diagnosis: string }>;
    insufficientData: boolean;
    configured: boolean;
  } | null>(null);

  onMount(async () => {
    api.get('/api/practice/status').then((s) => (srs = s)).catch(() => (srs = null));
    api.get('/api/pavlov/stats').then((s) => (pavlov = s)).catch(() => (pavlov = null));
    api.get('/api/blindspots').then((b) => (blindspots = b)).catch(() => (blindspots = null));
    try {
      stats = await api.get('/api/stats');
    } catch (err: any) {
      error = err?.message ?? 'Failed to load stats';
    } finally {
      loading = false;
    }
  });

  // Top 3 categories with the most projected-score upside, excluding ones with
  // no cold data yet (estimated at a neutral 0.5 — nothing actionable to show).
  let topHeadroom = $derived(
    stats?.projectedMock ? stats.projectedMock.categories.filter((c) => !c.estimated).slice(0, 3) : []
  );
</script>

<svelte:head>
  <title>Dashboard — Jeopardy! Training</title>
</svelte:head>

<svelte:window bind:innerWidth />

<div class="min-h-screen bg-gray-50 py-8 px-4">
  <div class="max-w-6xl mx-auto">
    <div class="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-4 mb-8">
      <h1 class="text-3xl font-bold text-jeopardy-blue">Dashboard</h1>
    </div>

    <div class="flex flex-wrap gap-3 mb-8">
      <a href="/pavlov" class="px-5 py-2.5 bg-jeopardy-gold text-jeopardy-blue font-semibold rounded-lg hover:bg-yellow-400 transition-colors">Pavlov Drill</a>
      <a href="/practice" class="px-5 py-2.5 bg-jeopardy-blue text-white font-semibold rounded-lg hover:bg-blue-800 transition-colors">Practice</a>
      <a href="/drill" class="px-5 py-2.5 bg-jeopardy-blue text-white font-semibold rounded-lg hover:bg-blue-800 transition-colors">Drill</a>
      <a href="/coryat" class="px-5 py-2.5 bg-jeopardy-blue text-white font-semibold rounded-lg hover:bg-blue-800 transition-colors">Coryat</a>
    </div>

    <!-- ============================ Pavlov ============================ -->
    {#if pavlov && pavlov.progress.deckTotal > 0}
      <StatsSection
        title="Pavlov"
        accent="gold"
        summary={{ due: pavlov.dueCount, newLeft: pavlov.newRemaining, reviewedToday: pavlov.reviewedToday, href: '/pavlov', label: 'Pavlov Drill' }}
        forecast={pavlov.forecast}
        deck={pavlov.deck}
        cold30d={pavlov.cold30d}
        cold={pavlov.cold}
        review={pavlov.review}
        coldCaption="First grade on each cue card only"
        reviewLabel="cue reviews"
        historySince={pavlov.historySince}
        daily={pavlov.dailyAccuracy}
        categories={pavlov.categoryBreakdown}
        {isMobile}
      >
        {#snippet readiness()}
          <PavlovProgressCard progress={pavlov!.progress} />
        {/snippet}
      </StatsSection>
    {/if}

    <!-- ======================= Standard Practice ======================= -->
    {#if loading}
      <div class="flex justify-center py-16">
        <div class="animate-spin rounded-full h-12 w-12 border-b-2 border-jeopardy-blue"></div>
      </div>
    {:else if error}
      <div class="px-4 py-3 bg-red-50 border border-red-200 text-red-700 rounded-lg">{error}</div>
    {:else if stats}
      <StatsSection
        title="Standard Practice"
        accent="blue"
        summary={srs ? { due: srs.dueCount, newLeft: srs.newRemaining, reviewedToday: srs.reviewedToday, href: '/practice', label: 'Practice' } : null}
        forecast={srs?.forecast ?? null}
        deck={srs?.deck ?? null}
        deckLinkBase="/cards?state="
        cold30d={stats.cold30d}
        cold={stats.cold}
        review={stats.review}
        coldCaption="First-attempt questions only — the number the Anytime Test measures"
        reviewLabel="SRS reviews"
        daily={stats.dailyAccuracy}
        categories={stats.categoryBreakdown}
        {isMobile}
      >
        {#snippet extras()}
          {#if srs?.adaptiveWeights && srs.adaptiveWeights.length > 0}
            {@const maxWeight = Math.max(...srs.adaptiveWeights.map((w) => w.weight))}
            <div class="mt-5 pt-4 border-t border-gray-100">
              <h3 class="text-sm font-semibold text-gray-600 mb-1">Focus areas</h3>
              <p class="text-xs text-gray-400 mb-3">
                Practice draws new clues where they're worth the most test points — weakness
                weighted by each category's share of the real Anytime Test. The bar and percentage
                show each category's share of your new clues, highest priority first.
              </p>
              <div class="flex flex-col gap-2.5 sm:gap-1.5">
                {#each srs.adaptiveWeights as w (w.category)}
                  <div class="flex flex-wrap sm:flex-nowrap items-center gap-x-3 gap-y-1 text-sm">
                    <span class="order-1 flex-1 sm:flex-none sm:w-52 truncate text-gray-700">{w.category}</span>
                    <span class="order-2 sm:order-3 shrink-0 sm:w-32 text-right text-xs text-gray-400">
                      {w.attempts > 0 ? `${Math.round(w.accuracy)}% right` : 'untried'} · {w.attempts} tries
                    </span>
                    <div class="order-3 sm:order-2 flex items-center gap-2 w-full sm:w-auto sm:flex-1">
                      <div class="flex-1 h-2 bg-gray-100 rounded-full overflow-hidden">
                        <div class="h-full bg-jeopardy-blue rounded-full" style="width: {maxWeight > 0 ? (w.weight / maxWeight) * 100 : 0}%"></div>
                      </div>
                      <span class="w-10 shrink-0 text-right text-xs font-semibold text-gray-600">{Math.round(w.weight * 100)}%</span>
                    </div>
                  </div>
                {/each}
              </div>
              <p class="mt-2 text-[11px] text-gray-400">
                "% right" counts every attempt (first tries and reviews)
                {srs.adaptiveWindow === 'all' ? 'across all time' : 'over the last 180 days'}, so it can
                differ from the cold-accuracy table below. Ranking discounts small samples — a category
                with a few bad tries sits below one that misses often over many.
              </p>
            </div>
          {/if}
        {/snippet}

        {#snippet afterSummary()}
          {#if blindspots && blindspots.configured}
            <a
              href="/blindspots"
              class="bg-white rounded-xl shadow-sm p-5 mb-8 flex items-center justify-between hover:bg-gray-50 transition-colors group block"
            >
              <div>
                <p class="font-semibold text-gray-800">Blind spots</p>
                {#if blindspots.packs.length > 0}
                  <p class="text-sm text-gray-500 mt-0.5">{blindspots.packs.slice(0, 3).map((p) => p.theme).join(' · ')}</p>
                {:else if blindspots.insufficientData}
                  <p class="text-sm text-gray-500 mt-0.5">Keep practicing — analysis unlocks after a few more misses.</p>
                {:else}
                  <p class="text-sm text-gray-500 mt-0.5">Analyze your recent misses for patterns.</p>
                {/if}
              </div>
              <span class="text-gray-400 group-hover:text-gray-600 text-lg">&rarr;</span>
            </a>
          {/if}
        {/snippet}

        {#snippet readiness()}
          <div class="flex-1 min-w-[200px] bg-white rounded-xl shadow p-6">
            <p class="text-sm font-medium text-gray-500 mb-1">Mock Test Readiness</p>
            {#if stats!.mockReadiness.tests.length > 0}
              <p class="text-3xl font-bold {(stats!.mockReadiness.latest ?? 0) >= stats!.mockReadiness.passLine ? 'text-green-600' : 'text-jeopardy-blue'}">
                {stats!.mockReadiness.latest}/50
              </p>
              <p class="text-xs text-gray-400 mt-1">Best {stats!.mockReadiness.best}/50 · pass line {stats!.mockReadiness.passLine} · <a href="/mock" class="text-jeopardy-blue hover:underline">take another →</a> · <a href="/mock/history" class="text-jeopardy-blue hover:underline">history →</a></p>
            {:else}
              <p class="text-sm text-gray-500 mt-1">No mocks yet.</p>
              <a href="/mock" class="text-sm font-semibold text-jeopardy-blue hover:underline">Take your first mock test →</a>
            {/if}
          </div>
        {/snippet}

        {#snippet belowTiles()}
          {#if stats!.projectedMock}
            <div class="bg-white rounded-xl shadow p-6 mb-8">
              <p class="text-sm font-medium text-gray-500 mb-1">Projected Anytime Test Score</p>
              <p class="text-4xl font-extrabold {stats!.projectedMock.score >= 35 ? 'text-green-600' : stats!.projectedMock.score >= 30 ? 'text-amber-500' : 'text-red-500'}">
                {stats!.projectedMock.score.toFixed(1)}/50
              </p>
              <p class="text-xs text-gray-400 mt-1">
                Modeled from each category's cold accuracy weighted by its share of the real test · pass line {stats!.projectedMock.passLine}.
              </p>
              {#if topHeadroom.length > 0}
                <div class="mt-4 pt-3 border-t border-gray-100 flex flex-col gap-1.5">
                  <p class="text-xs font-semibold text-gray-500 uppercase tracking-wide mb-1">Biggest opportunities</p>
                  {#each topHeadroom as c (c.category)}
                    <div class="flex items-center justify-between text-sm">
                      <span class="text-gray-700 truncate">{c.category}</span>
                      <span class="font-semibold text-jeopardy-blue shrink-0 ml-3">+{c.headroom.toFixed(1)} pts available</span>
                    </div>
                  {/each}
                </div>
              {/if}
            </div>
          {/if}
        {/snippet}
      </StatsSection>
    {/if}
  </div>
</div>
