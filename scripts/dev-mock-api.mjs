#!/usr/bin/env node
// Mock API for verifying dashboard changes locally (no credentials needed).
// Usage:  node scripts/dev-mock-api.mjs            (port 3999)
//         MOCK_EMPTY=1 node scripts/dev-mock-api.mjs   (Pavlov with no review history)
// Then:   cd frontend && VITE_API_PROXY=http://127.0.0.1:3999 npm run dev
import http from 'node:http';

const PORT = Number(process.env.PORT ?? 3999);
const EMPTY = process.env.MOCK_EMPTY === '1';

const CATS = [
  'Literature & Language', 'Geography & Exploration', 'History & Politics', 'Science & Nature',
  'Film, TV & Pop Culture', 'Philosophy, Religion & Society', 'Music & Performing Arts',
  'Miscellaneous', 'Technology & Engineering', 'Mathematics & Logic', 'Business & Economics',
  'Sports & Games', 'Art & Culture',
];
const kind = (total, correct) => ({ total, correct, accuracy: total ? (correct / total) * 100 : 0 });
const split = (c, t, cc, ct, rt, rc) => ({
  category: c, total: t, correct: cc, accuracy: (cc / t) * 100,
  coldTotal: ct, coldCorrect: Math.round(ct * 0.5), coldAccuracy: 50 + (c.length % 30),
  reviewTotal: rt, reviewCorrect: rc, reviewAccuracy: (rc / rt) * 100,
});
const isoDay = (offset) => {
  const d = new Date();
  d.setDate(d.getDate() + offset);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
};
const daily = (n) =>
  Array.from({ length: n }, (_, i) => ({
    date: isoDay(i - n + 1), total: 40, correct: 28, accuracy: 70,
    coldTotal: 15, coldCorrect: 8, coldAccuracy: 45 + ((i * 7) % 25),
    reviewTotal: 25, reviewCorrect: 22, reviewAccuracy: 80 + ((i * 3) % 15),
  }));
const forecast = (base) => Array.from({ length: 14 }, (_, i) => ({ date: isoDay(i), count: base + ((i * 37) % 60) }));

const routes = {
  '/api/auth/me': { user: { id: 1, username: 'ebertx', email: 'ebertx@example.com', role: 'admin' } },
  '/api/blindspots': { configured: true, insufficientData: false, packs: [
    { id: 1, theme: 'Mythology', diagnosis: '' }, { id: 2, theme: 'Opera', diagnosis: '' }, { id: 3, theme: 'Fine Arts', diagnosis: '' } ] },
  '/api/stats': {
    overall: kind(4321, 3300), cold: kind(2100, 1050), review: kind(2221, 1800), cold30d: kind(220, 118),
    mockReadiness: { tests: [{ id: 1, completedAt: '2026-07-20T00:00:00Z', score: 18 }, { id: 2, completedAt: '2026-07-23T00:00:00Z', score: 21 }], best: 21, latest: 21, passLine: 35 },
    projectedMock: { score: 27.4, passLine: 35, categories: CATS.map((c, i) => ({ category: c, share: 0.08, coldAccuracy: 45 + i, contribution: 2, headroom: 3 - i * 0.2, estimated: false })) },
    categoryBreakdown: CATS.map((c) => split(c, 300, 200, 150, 150, 120)),
    dailyAccuracy: daily(30),
  },
  '/api/practice/status': {
    dueCount: 42, newRemaining: 30, reviewedToday: 18, forecast: forecast(20),
    adaptiveWeights: CATS.slice(0, 6).map((c, i) => ({ category: c, attempts: 300 - i * 30, accuracy: 45 + i * 5, weight: 0.3 - i * 0.04 })),
    adaptiveWindow: '180d',
    deck: { learning: 120, maturing: 900, mastered: 1400, struggling: 60, total: 2480, delta: { since: isoDay(-7), learning: 5, maturing: 12, mastered: 18, struggling: -2 } },
  },
  '/api/pavlov/stats': EMPTY
    ? {
        overall: kind(0, 0), cold: kind(0, 0), review: kind(0, 0), cold30d: kind(0, 0), historySince: null,
        dailyAccuracy: [], categoryBreakdown: [], reviewedToday: 0, dueCount: 124, newRemaining: 40, forecast: forecast(30),
        deck: { learning: 24, maturing: 402, mastered: 585, struggling: 0, banished: 86, total: 1097, delta: null },
        progress: { deckTotal: 4765, touched: 1097, touchedPct: 23.02, trailingPerDay: 11, targetDate: '2026-12-31', daysLeft: 108, requiredPerDay: 34, pastTarget: false, projectedFinish: '2027-08-15', daysAhead: -226 },
      }
    : {
        overall: kind(1200, 960), cold: kind(400, 180), review: kind(800, 780), cold30d: kind(400, 180), historySince: '2026-09-16T04:00:00Z',
        dailyAccuracy: daily(12), categoryBreakdown: CATS.map((c) => split(c, 90, 70, 30, 60, 55)),
        reviewedToday: 96, dueCount: 124, newRemaining: 40, forecast: forecast(30),
        deck: { learning: 24, maturing: 402, mastered: 585, struggling: 3, banished: 86, total: 1100, delta: { since: isoDay(-7), learning: -10, maturing: 40, mastered: 55, struggling: 1 } },
        progress: { deckTotal: 4765, touched: 1100, touchedPct: 23.08, trailingPerDay: 36, targetDate: '2026-12-31', daysLeft: 108, requiredPerDay: 34, pastTarget: false, projectedFinish: '2026-12-26', daysAhead: 6 },
      },
};

http
  .createServer((req, res) => {
    const path = req.url.split('?')[0];
    const body = routes[path];
    res.setHeader('Content-Type', 'application/json');
    if (!body) {
      res.statusCode = 404;
      return res.end(JSON.stringify({ error: `no mock for ${path}` }));
    }
    res.end(JSON.stringify(body));
  })
  .listen(PORT, '127.0.0.1', () => console.log(`mock api on http://127.0.0.1:${PORT} (empty=${EMPTY})`));
