// Shared shapes for /api/stats, /api/practice/status and /api/pavlov/stats.

export interface KindStat {
  total: number;
  correct: number;
  accuracy: number;
}

export interface SplitStat {
  total: number;
  correct: number;
  accuracy: number;
  coldTotal: number;
  coldCorrect: number;
  coldAccuracy: number;
  reviewTotal: number;
  reviewCorrect: number;
  reviewAccuracy: number;
}

export interface CategoryStat extends SplitStat {
  category: string | null;
}

export interface DailyStat extends SplitStat {
  date: string;
}

export interface ForecastDay {
  date: string;
  count: number;
}

export interface DeckDelta {
  since: string;
  learning: number;
  maturing: number;
  mastered: number;
  struggling: number;
}

export interface DeckStats {
  learning: number;
  maturing: number;
  mastered: number;
  struggling: number;
  banished?: number;
  total: number;
  delta: DeckDelta | null;
}

export interface SummaryStrip {
  due: number;
  newLeft: number;
  reviewedToday: number;
  href: string;
  label: string;
}

export interface PavlovProgress {
  deckTotal: number;
  touched: number;
  touchedPct: number;
  trailingPerDay: number;
  targetDate: string;
  daysLeft: number;
  requiredPerDay: number;
  pastTarget: boolean;
  projectedFinish: string | null;
  daysAhead: number | null;
  hooksSeen: number;
  hooksTotal: number;
}

export interface PavlovStats {
  overall: KindStat;
  cold: KindStat;
  review: KindStat;
  cold30d: KindStat;
  historySince: string | null;
  dailyAccuracy: DailyStat[];
  categoryBreakdown: CategoryStat[];
  reviewedToday: number;
  dueCount: number;
  newRemaining: number;
  forecast: ForecastDay[];
  deck: DeckStats;
  progress: PavlovProgress;
}

/** Local calendar date → 'YYYY-MM-DD' (matches backend user-timezone bucketing). */
export function localDateKey(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
}

/** 'YYYY-MM-DD' → short label like "Dec 31" without UTC-shift surprises. */
export function fmtDate(iso: string): string {
  return new Date(iso + 'T00:00:00').toLocaleDateString([], { month: 'short', day: 'numeric' });
}
