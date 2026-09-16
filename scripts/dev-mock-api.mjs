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
  '/api/mock-test/history': { passLine: 35, best: 21, tests: [
    { id: 8, completedAt: '2026-09-15T23:46:22Z', score: 20, missKinds: { unknown: 27, slow: 2, wording: 0 } },
    { id: 5, completedAt: '2026-07-23T22:52:20Z', score: 21, missKinds: { unknown: 20, slow: 6, wording: 3 } },
    { id: 3, completedAt: '2026-07-20T22:28:34Z', score: 18, missKinds: { unknown: 0, slow: 0, wording: 0 } },
  ] },
  '/api/blindspots': { configured: true, insufficientData: false, packs: [
    { id: 1, theme: 'Mythology', diagnosis: '' }, { id: 2, theme: 'Opera', diagnosis: '' }, { id: 3, theme: 'Fine Arts', diagnosis: '' } ] },
  '/api/activity': { streak: 3, activeLast28: 11, days: Array.from({ length: 28 }, (_, i) => ({ date: isoDay(i - 27), active: [5, 6, 8, 12, 13, 16, 19, 20, 25, 26, 27].includes(i) })) },
  '/api/pavlov/drill/next': { done: false, isNew: false, dueCount: 124, newRemaining: 40, card: { answerId: 1, answerNorm: 'visigoths', category: 'History & Politics', phrases: [], hookId: 3, hookRank: 3, cue: 'ruled Spain from Toledo until 711' } },
  '/api/pavlov/drill/check': { correct: null, answer: 'the Visigoths', answerNorm: 'visigoths', forms: ['the Visigoths', 'Visigoths'], servedHookId: 3,
    hooks: [
      { id: 1, rank: 1, cue: 'split from the Ostrogoths, "western" Goths', support: 9, source: 'cue', seen: 2, lastWrongAt: null },
      { id: 2, rank: 2, cue: 'Alaric sacks Rome, 410', support: 5, source: 'cue', seen: 1, lastWrongAt: isoDay(-1) + 'T18:00:00Z' },
      { id: 3, rank: 3, cue: 'ruled Spain from Toledo until 711', support: 6, source: 'model', seen: 0, lastWrongAt: null },
      { id: 4, rank: 4, cue: 'Alaric II · Vouillé · Franks, 507', support: 2, source: 'model', seen: 0, lastWrongAt: null },
    ],
    exampleClue: { clue: 'In 711 a Muslim army defeated Roderick, the last king of these people in Spain', category: 'VICTORY IS OURS', airDate: '2004-09-24' },
    examples: [] },
  '/api/pavlov/drill/grade': { state: 'learning', due: new Date().toISOString(), intervalDays: 0, requeueInSession: true },
  '/api/pavlov/hooks/3/drop': { status: 'dropped' },
  '/api/pavlov/entity/question/1': { answerId: 1, answer: 'the Visigoths', forms: ['the Visigoths', 'Visigoths'], hooks: [
      { id: 1, rank: 1, cue: 'split from the Ostrogoths, "western" Goths', support: 9, source: 'cue', seen: 2, lastWrongAt: null },
      { id: 3, rank: 3, cue: 'ruled Spain from Toledo until 711', support: 6, source: 'model', seen: 0, lastWrongAt: null } ] },
  '/api/pavlov/answers': { answers: [
    { id: 1, answer: 'the Visigoths', category: 'History & Politics', forms: ['the Visigoths', 'Visigoths'], vetted: false, suspended: false, phrases: [],
      hooks: [ { id: 1, rank: 1, cue: 'split from the Ostrogoths, "western" Goths', keyGram: 'ostrogoth', support: 9, source: 'cue', status: 'active' },
               { id: 3, rank: 3, cue: 'ruled Spain from Toledo until 711', keyGram: '711', support: 6, source: 'model', status: 'active' },
               { id: 5, rank: 5, cue: null, keyGram: 'roman', support: 2, source: 'mined', status: 'active' } ] },
    { id: 2, answer: 'Jean Sibelius', category: 'Music & Performing Arts', forms: ['(Jean) Sibelius', 'Jean Sibelius', 'Sibelius'], vetted: true, suspended: false, phrases: [],
      hooks: [ { id: 7, rank: 1, cue: 'Finnish composer of "Finlandia"', keyGram: 'finlandia', support: 21, source: 'both', status: 'active' } ] },
  ] },
  '/api/pavlov/hooks/1': { id: 1, cue: 'split from the Ostrogoths, "western" Goths', keyGram: 'ostrogoth', grams: ['ostrogoth', 'goth split'], examples: [ { clue: 'Circa 370 A.D., the Goths split into 2 tribes, the Ostrogoths & these people', category: 'ANCIENT TIMES', airDate: '1987-10-14' } ] },
  '/api/pavlov/hooks/3': { id: 3, cue: 'ruled Spain from Toledo until 711', keyGram: '711', grams: ['711', 'spain'], examples: [ { clue: 'In 711 a Muslim army defeated Roderick, the last king of these people in Spain', category: 'VICTORY IS OURS', airDate: '2004-09-24' } ] },
  '/api/pavlov/hooks/5': { id: 5, cue: null, keyGram: 'roman', grams: ['roman'], examples: [] },
  '/api/pavlov/hooks/7': { id: 7, cue: 'Finnish composer of "Finlandia"', keyGram: 'finlandia', grams: ['finlandia', 'finnish compos'], examples: [] },
  '/api/admin/pavlov/status': { running: false, pending: 0, active: 13062, dropped: 900, hooks: { total: 14200, labeled: 13100, unlabeled: 1100, vetted: 610, entitiesPending: 0 } },
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
        progress: { deckTotal: 4765, touched: 1097, touchedPct: 23.02, trailingPerDay: 11, targetDate: '2026-12-31', daysLeft: 108, requiredPerDay: 34, pastTarget: false, projectedFinish: '2027-08-15', daysAhead: -226, hooksSeen: 0, hooksTotal: 0 },
      }
    : {
        overall: kind(1200, 960), cold: kind(400, 180), review: kind(800, 780), cold30d: kind(400, 180), historySince: '2026-09-16T04:00:00Z',
        dailyAccuracy: daily(12), categoryBreakdown: CATS.map((c) => split(c, 90, 70, 30, 60, 55)),
        reviewedToday: 96, dueCount: 124, newRemaining: 40, forecast: forecast(30),
        deck: { learning: 24, maturing: 402, mastered: 585, struggling: 3, banished: 86, total: 1100, delta: { since: isoDay(-7), learning: -10, maturing: 40, mastered: 55, struggling: 1 } },
        progress: { deckTotal: 4765, touched: 1100, touchedPct: 23.08, trailingPerDay: 36, targetDate: '2026-12-31', daysLeft: 108, requiredPerDay: 34, pastTarget: false, projectedFinish: '2026-12-26', daysAhead: 6, hooksSeen: 1830, hooksTotal: 3400 },
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
