# Jeopardy Objects — Entities, Hooks, and the Vetted Pavlov Import — Design

**Date:** 2026-09-16
**Status:** Approved (Christian: approach A, short hook cue on the prompt,
all five sections approved in chat)
**Supersedes:** `2026-09-15-answer-sheets-and-activity-design.md` for answer
sheets and fact cards (the active-days strip stays). Extends
`2026-07-22-pavlov-cues-v2-design.md` (mining) and
`2026-07-22-pavlov-answer-cards-design.md` (per-answer cards).

## Why

Three findings from the 2026-09-16 review of the flat mock scores
(18 / 21 / 20):

1. **Fact cards were random.** The 36 fact cards generated so far are
   biography trivia (Ben Jonson's birth year, Edison's first factory date,
   Nostradamus's death date) and they drill the reverse direction
   ("title character's wife → Desdemona" is a Desdemona card). The model
   chose the angles, so it produced an encyclopedia entry, not hooks.
2. **The hooks are already in the corpus.** The 21 Visigoths clues fall into
   four angles — split from the Ostrogoths (9 clues), Alaric sacks Rome 410
   (5), ruled Spain from Toledo until 711 (6), Alaric II at Vouillé 507 (2).
   The card covered the first two; the mock clue used the third. The
   existing n-gram table separates the angles with no model involved.
3. **The deck is built on strings, not entities.** Generation groups clues by
   the exact normalized response, so "(Edvard) Grieg" (19 clues),
   "Edvard Grieg" (19) and "Grieg" (11) are three answers, each below the
   mining bar. Grieg — 52 clues — is not in the deck. Nor are Sibelius,
   Debussy, Holst, Dvořák, Elgar, Mahler, Berlioz, Whistler, Hopper or
   Sargent. 12,567 clues use the "(First) Last" form alone. Of 655 responses
   on the community JBoard Pavlov lists, 605 are in the corpus but only 280
   are in the deck.

The ChatGPT coaching conversation Christian shared says the same thing from
the other side: make the fundamental unit an **entity** with 3–8 **hooks**,
generate cues from each direction, and turn every miss into a small
neighbourhood ("1→5 learning"), not a fact list.

So: the entity becomes the card, hooks are mined from the entity's own clues,
the community-vetted Pavlov lists are imported as labels and as a priority
tier, and the sheet/fact-card machinery is removed.

Chosen approach (A of three): corpus clustering finds the hooks; the model
only renders a short label, under the same grounding gate v2.1 uses for cue
phrases. Rejected: B, model-authored hooks with clue citations (the model
picks the angles and drifts back to biography); C, raw gram stems as cues
(not human-answerable — the v1 lesson).

## 1. Entity resolution and data model

### Resolution rules

Amended during execution after a dry run over the live corpus (155,851
response forms): the original "parenthetical license" merged different
people and places under one surname (George, Denzel and Booker T.
Washington plus the place; London with Jack London; Chicago with the
University of Chicago). The rules below replace it.

Applied to `jeopardy_questions.question` (the response) for every clue with
`archived = false`:

1. **Parentheticals.** A parenthetical that LEADS the response and whose
   content is name-like (letters, spaces, `.`, `-`, `'`, `"`, `&`) is part
   of the name: "(George) Washington" → George Washington, "(Sir Edward)
   Elgar" → Sir Edward Elgar, "(University of) Chicago" → University of
   Chicago. Every other parenthetical is an annotation and is dropped:
   "Andrew Jackson (Old Hickory)" → Andrew Jackson, "Mexico (Mexico City)"
   → Mexico, "(1 of) Spain (or Portugal)" → Spain. Quote characters are
   never touched in corpus responses (the corpus stores titles as
   `\"The Raven\"`); only the vetted-list import strips quoted nicknames.
2. **Key = the full name.** `entity_key` = strip parentheticals as above →
   strip one leading `Sir`/`Dame` → the existing article-strip
   normalisation. "(Edvard) Grieg" and "Edvard Grieg" → `edvard grieg`;
   "Grieg" → `grieg`. A response that strips to nothing (or a bare article)
   keeps its plain string norm as its key.
3. **Bare-surname absorption, three guards** (amended again after the
   first live Resolve run: a count-dominance guard both let lowercase
   homonyms through — Shirley Temple absorbed "temple", Henry Clay "clay" —
   and blocked the Beethoven class). A single-token key `s` is absorbed into
   `f s` only when (a) exactly ONE first name `f` is licensed for `s` by some
   "(F) S" form; (b) the count-weighted dominant category of `s` equals that
   of `f s` and is non-empty (London: Geography ≠ Jack London: Literature →
   separate; Beethoven and Ludwig van Beethoven: both Music → merge); and
   (c) only eligible forms move — a single token, capitalised, with no
   leading article, parenthesis or quote. "Temple" joins Shirley Temple;
   "temple", "the temple" and "a Caesar" stay their own entity. Each form
   carries the dominant `classifier_category` of its clues for guard (b).
4. **Display** = the most frequent stripped form among the forms whose own
   key equals the entity key ("France", not "the France"; "Sir Edward Elgar"
   because the honorific forms outnumber "Edward Elgar"). Frequency = the
   sum over merged forms; `forms` = raw strings by count desc.

Acceptance test: the 655 JBoard responses (see §2), plus the dry-run harness
over the live corpus during rollout: no bare surname may absorb more than
one person, and the 50 highest-frequency merges are hand-reviewed.

### Migration 0016

```sql
ALTER TABLE jeopardy_questions ADD COLUMN IF NOT EXISTS entity_norm TEXT;
CREATE INDEX IF NOT EXISTS idx_jq_entity_norm ON jeopardy_questions (entity_norm);

ALTER TABLE pavlov_answers
  ADD COLUMN IF NOT EXISTS forms  TEXT[]  NOT NULL DEFAULT '{}',
  ADD COLUMN IF NOT EXISTS vetted BOOLEAN NOT NULL DEFAULT false;

CREATE TABLE IF NOT EXISTS pavlov_hooks (
  id         SERIAL PRIMARY KEY,
  answer_id  INTEGER NOT NULL REFERENCES pavlov_answers(id) ON DELETE CASCADE,
  key_gram   TEXT    NOT NULL,          -- cluster's most distinctive gram; stable identity
  rank       INTEGER NOT NULL,          -- 1 = most clues
  cue        TEXT,                      -- short human label; NULL = unlabeled, not drillable
  grams      TEXT[]  NOT NULL DEFAULT '{}',
  clue_ids   INTEGER[] NOT NULL DEFAULT '{}',
  support    INTEGER NOT NULL DEFAULT 0,
  source     TEXT NOT NULL DEFAULT 'mined' CHECK (source IN ('mined','vetted','cue','model','both')),
  status     TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','dropped')),
  model      TEXT NOT NULL DEFAULT '',
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (answer_id, key_gram)
);
CREATE INDEX IF NOT EXISTS idx_pavlov_hooks_answer ON pavlov_hooks (answer_id, status);

ALTER TABLE pavlov_reviews
  ADD COLUMN IF NOT EXISTS hook_id INTEGER REFERENCES pavlov_hooks(id) ON DELETE SET NULL;

-- removals (answer sheets / fact cards)
DELETE FROM pavlov_answers WHERE kind = 'fact';          -- cascades cards + reviews
DROP TABLE IF EXISTS answer_sheets;
ALTER TABLE pavlov_answers DROP COLUMN IF EXISTS kind, DROP COLUMN IF EXISTS parent_norm;
DROP INDEX IF EXISTS idx_pavlov_answers_kind_parent;
ALTER TABLE users DROP COLUMN IF EXISTS pavlov_auto_facts;
```

`source` meaning: `mined` = cluster without a label yet; `cue` = labelled by
an existing v2 cue; `model` = labelled by the render call; `vetted` = a
JBoard pair that matched no cluster; `both` = a cluster labelled by a
JBoard pair.

- `pavlov_answers` stays the entity table: one row per entity, `answer_norm`
  = entity key, `forms` = merged response strings. `phrases` /
  `phrase_tiers` remain through the transition (§5) and are dropped later.
- All Pavlov mining keys on `entity_norm`. `pavlov_clue_ngrams.answer_norm`
  is rewritten only for clues whose key changed (~13k of 7.2M rows).
- `pavlov_cards` unchanged: SRS state is per entity. When merged forms each
  have a card for a user, the card with the most `reps` survives; the
  others' `pavlov_reviews` rows are re-pointed to it and the losers deleted.
- Per-hook exposure is derived from `pavlov_reviews.hook_id`; no separate
  exposure table.

## 2. Hook mining and the vetted import

### Mining, per entity

Input: the entity's clues (via `entity_norm`) and their grams in
`pavlov_clue_ngrams`.

- **Seed grams:** support ≥ 2 within the entity AND distinctive in the
  corpus: unigram corpus frequency ≤ 3000, bigram ≤ 300
  (`HOOK_UNIGRAM_MAX_DF`, `HOOK_BIGRAM_MAX_DF`, constants beside the v2
  thresholds; amended from 500 / 100 during planning — "spain" has corpus
  frequency 1,254 and is the Visigoths' Spain angle). For Visigoths this
  keeps alaric, ostrogoth, sack rome, 410, 711, spain, 507, alaric ii and
  drops "people" (6,484), "king" (7,762), "made" (10,924).
- **Clue assignment weight:** a clue joins the cluster whose grams present
  in it carry the most inverse-document-frequency weight
  (`ln(corpus_clues / df)`), ties to the larger cluster — so common-but-
  relevant grams contribute less than rare ones without being dropped.
- **Seed cap:** only the `HOOK_MAX_SEEDS = 60` seed grams with the highest
  support × idf take part in clustering (amended during execution: the
  pairwise merge loop is quadratic in seeds, and a 500-clue entity can have
  over a thousand recurring grams; hooks are capped at 8, so the top 60 by
  weight carry every angle that could rank).
- **Clustering:** each seed gram starts a cluster with its clue set. Two
  clusters merge when Jaccard(clue sets) ≥ 0.5 or one set contains the
  other. Then every clue is assigned to the cluster whose grams it matches
  most (by idf weight, above). Clusters are ranked by member count, capped at 8
  (`HOOK_MAX_PER_ENTITY`), dropped below 2 members (`HOOK_MIN_SUPPORT`).
  Clues matching no cluster form no hook. An entity with one cluster is a
  one-hook card, which is what every card is today.
- **Identity:** `(answer_id, key_gram)`, key gram = the cluster's gram with
  the highest support × idf (ties alphabetically first; amended during
  execution from "lowest corpus frequency" — a gram that recurs across the
  angle's clues is a more stable identity than a one-off rare term).
  Regeneration upserts on it so hook ids, and therefore exposure history,
  survive reruns.

### Labels (priority order)

1. **Vetted:** a JBoard pair whose stemmed cue words hit the cluster's grams.
   The human cue is used verbatim; `source = 'both'`.
2. **Existing cue:** an active v2 `pavlov_cues` row (`status = 'active'`,
   `tier = 'standard'`) for the entity whose `cue_stem` is among the
   cluster's grams. `cue_display` reused, no model call; `source = 'cue'`.
3. **Model:** batched render as in v2.1 (`render_prompts` /
   `parse_render_response`), given the grams and up to 5 member clues,
   asked for a 2–8 word cue. Gates unchanged: every content word grounded
   in a member clue (`display_grounded`), no answer leak
   (`phrase_leaks_answer`), no hedges. `source = 'model'`.

A rejected label leaves `cue = NULL`. Unlabeled hooks are not drilled and
not shown on the map; they are retried on the next hooks run and are visible
on `/pavlov/list` as "unlabeled" with their key gram.

Amended 2026-09-17 after the first live run (label quality review):

- **No keep flag.** The render prompt asked the model for a `keep` boolean
  ("false when the clues do not share a real angle"). gpt-4o-mini flipped it
  for whole batches at a time: 197 of 801 batches came back all-false with
  good cues attached (Napoleon's Josephine / St. Helena / 1804, Curie's
  Sorbonne / Sklodowska), stranding a third of all hooks. The flag is gone;
  the grounding, leak, length and hedge gates are the only gates.
- **Tidy pass for cue-sourced labels.** A label reused from a v2 cue is a
  phrase clipped from one clue and often a fragment ("of \"Sense &
  Sensibility\" who"). Each is sent once to the model as a `draft` with the
  same gates, grounding widened to include the draft; a rejected rewrite
  keeps the draft. `source` stays `cue`, `model` records the tidy.
- **Frame cap.** A cluster keyed by an identity stem — profession,
  nationality, category noun (`FRAME_GRAMS`: poet, compos, english,
  reptil, …) — is the frame, not an angle ("english playwright", "type of
  reptile"). One per entity is kept, by support; the rest are deleted right
  after mining, before any label is spent.
- **Duplicate merge.** After labeling, two active labeled hooks of one entity
  whose cues share a content token (≥ 5 chars, plural folded, frame and
  connective words excluded — `cue_content_tokens`) are the same angle
  twice (three cubism hooks on Picasso). The loser folds into the keeper
  (vetted first, then support): clue ids and grams unioned, reviews
  re-pointed, row deleted.
- **Parallel batches.** `HOOK_LABEL_PARALLEL = 4` label batches in flight;
  each is claimed before its call, so a failure retries on the next run.

### Vetted import

- Source: the JBoard "Pavlov revival" thread (t=343, 8 pages) plus t=2202
  and t=7592, scraped 2026-09-16. About 670 `cue = response` pairs across
  opera & classical, art, mythology, explorers, saints, royalty, sports and
  countries. Committed to the repo as `backend/data/pavlovs-jboard.tsv`
  with columns `cue, response, domain, source_url` so the import is
  reproducible. (Raw scrape kept at
  `.playwright-mcp/jboard-pavlovs.md` during implementation; not committed.)
- Amended 2026-09-17: a list line joins several pavlovs for one response
  with " & " ("hudson river & hudson bay & killed in mutiny…"); each piece
  is its own cue (`vetted_cue_pieces`). Pieces that are scrape noise are
  dropped: all-caps nationality shorthand (NORWAY), unbalanced quotes (a
  line clipped mid-title), prose over 80 chars. A piece that leaks the
  answer ("pike's peak" for Zebulon Pike) is skipped. Each hooks run clears
  the previous run's vetted labels (`both` → unlabeled, stale vetted-only
  rows deleted) so a re-cut list replaces them; a cluster takes at most one
  vetted cue per run, later matching pieces become their own hooks.
- Each response resolves to an entity key with the §1 rules. A matched pair
  either labels a cluster (`both`) or becomes its own hook
  (`source = 'vetted'`, `support` = member clues found by stem search,
  possibly 0, ranked after mined hooks). Unmatched responses are written to
  the job log for hand review.
- An entity named by a vetted pair but absent from the deck is inserted with
  `vetted = true`, bypassing `MIN_ANSWER_FREQ`.

### Ordering

`pick_new_card` within the sampled category orders
`vetted DESC, answer_freq DESC, score DESC, id`. Roughly 500 vetted entities
at 50/day is the first ten days.

### Cost

~4,800 entities × ~3 hooks; after cue reuse an estimated 5–8k model labels,
batched 20 per gpt-4o-mini call. A few dollars, run once.

## 3. Drill and SRS

- Scheduling stays per entity. Ratings, lapses and banish are unchanged.
- **Hook selection** when a card is served, over the entity's labeled
  active hooks: fewest exposures by this user first; then the hook whose
  last rating was `wrong`; then `rank`. Each review logs `hook_id`.
- **Coverage guard:** while the entity has any labeled hook this user has
  never seen, the computed interval is capped at 7 days
  (`HOOK_COVERAGE_CAP_DAYS`) regardless of rating. Once every hook has been
  seen, intervals grow normally.
- **No pause on Wrong.** The hook map is on the reveal, so the reading
  happens before grading. The paused state, `e` and `d` bindings go away.
- **`x` on the reveal** drops the served hook (`status = 'dropped'`,
  deck-level like banish). `/pavlov/list` can drop and restore hooks.
- **Fallback:** an entity with no labeled active hook is served its legacy
  `phrases` exactly as today (`hook_id` NULL in the review).
- **Progress:** deck progress, pace and allowance still count entities. The
  progress card gains **hook coverage** = hooks seen at least once / labeled
  hooks on touched entities.
- **Practice:** on a miss whose response resolves to a deck entity, the
  teaching pause shows that entity's hook map. Otherwise nothing extra.

### API

| route | change |
|---|---|
| `drill_next` | response gains `hookId`, `hookRank`; the `cue` is the hook's cue (or legacy phrases when falling back) |
| `drill_check` | reveal gains `forms` (flat, beside the existing `answer`), `hooks [{id, rank, cue, support, source, seen, lastWrongAt}]`, `servedHookId`, `exampleClue {clue, category, airDate}` |
| `drill_grade` | request gains `hookId` (nullable); `factsAdded` removed |
| `POST /api/pavlov/hooks/{id}/drop`, `/restore` | curation |
| `GET /api/pavlov/answers` | rows gain `forms`, `vetted`, `hooks[...]` incl. unlabeled and dropped |
| `GET /api/pavlov/stats` | `progress` gains `hooksSeen`, `hooksTotal` |
| `GET /api/sheet/*`, `POST /api/pavlov/facts` | removed |
| preferences | `pavlovAutoFacts` removed |
| admin | `POST /api/admin/pavlov/resolve`, `POST /api/admin/pavlov/hooks` (resumable, like generate) |

## 4. UI

**Drill (`/pavlov`)** — prompt unchanged in shape (category, countdown, one
short cue). Reveal:

```
the Visigoths                          Visigoths · the Visigoths
● split from the Ostrogoths, "western" Goths      9
● Alaric sacks Rome, 410                          5   ✗ missed 9/15
▶ ruled Spain from Toledo until 711               6   ← this one
○ Alaric II · Vouillé · Franks, 507               2
e.g. "In 711 a Muslim army defeated Roderick, the last king of these people in Spain"
                                       1 wrong · 2 got it · 3 too easy · x drop hook · b banish
```

Filled dot = seen, hollow = not yet, arrow = served hook, ✗ = date of last
Wrong on that hook. Forms in muted text. Legacy-phrase fallback renders the
current reveal.

**List (`/pavlov/list`)** — each entity expands to forms, hooks in rank order
with label source, member count and up to three example clues; dropped hooks
dimmed with restore; unlabeled hooks shown with key gram; vetted-only filter.

**Practice** — `AnswerSheet` replaced by the hook map (light variant, no
arrow) inside the existing teaching pause.

**Dashboard** — progress card gains the hook-coverage line. Active-days strip
unchanged.

**Settings** — auto-add fact cards checkbox removed.

**Component** — `HookMap.svelte` shared by drill reveal, practice pause and
list; props: `entity`, `hooks`, `servedHookId?`, `dark`.

## 5. Migration, removal, testing, rollout

**Jobs** (admin-triggered, resumable like `generate`):

1. **Resolve:** compute `entity_norm` for every clue; merge `pavlov_answers`
   rows into entities (forms, summed `answer_freq`, display name);
   consolidate duplicate cards per user (keep max `reps`, re-point reviews,
   delete losers); rewrite `pavlov_clue_ngrams.answer_norm` for changed
   clues.
2. **Hooks:** cluster, label (vetted → cue → model), upsert on
   `(answer_id, key_gram)`; runs the vetted import first from
   `backend/data/pavlovs-jboard.tsv`.

**Transitional fallback:** the drill serves legacy phrases for any entity
without a labeled hook, so the deck never goes empty mid-rollout. A later
migration drops `phrases` / `phrase_tiers` once coverage is near complete.

**Code removed:** `backend/src/sheets.rs`, `routes/sheet.rs`,
`routes/pavlov_facts.rs`, the auto-facts path in `drill_grade`,
`AppState.sheet_inflight` / `sheet_failed`, `AnswerSheet.svelte`, the `e` /
`d` bindings and paused state in `/pavlov`, the sheet fetch in `/practice`,
the settings checkbox, `scripts/verify-sheets.sql`, the sheet/facts routes
in `scripts/dev-mock-api.mjs`.

**Testing.**

- Unit (pure Rust): resolver — positive (Grieg, Elgar, Debussy variants) and
  negative (Mexico/New Mexico, London/Jack London, Washington/Denzel
  Washington, Edward Grieg); clustering on a Visigoths fixture → four hooks
  in the right order; Jaccard merge, cap 8, min 2; key-gram choice; hook
  selection order (unseen → last wrong → rank); 7-day cap; vetted cue-to-
  cluster matching.
- `scripts/verify-objects.sql`: JBoard coverage before/after (280 today —
  the after number is reported, not guessed); the 50 highest-frequency
  merges for hand review; no `(user_id, entity)` with two cards; hooks-per-
  entity distribution; unlabeled count; every `pavlov_reviews.hook_id`
  belongs to the review's entity.
- Visual via `scripts/dev-mock-api.mjs`: reveal map, list expansion,
  practice pause, progress line, at 1280 and 390 wide.

**Rollout order.** Apply 0016 → push main over SSH → Watchtower → run
resolve → run verify, review the merge list → run hooks → spot-check
`/pavlov/list` → drill. Progress numbers shift when entities merge and
vetted ones are added; the pace card recomputes on its own.

**Known trade-offs.** Misspelled first names don't merge. A hook's exposure
history resets if its key gram changes on regeneration. Practice misses on
answers outside the deck get nothing. The 36 fact cards and 105 cached
sheets are deleted.

## Out of scope

Per-hook SRS (would multiply the deck 3–5× and sink the end-of-year goal);
Wikidata or other external entity resolution; the AnkiWeb / J!Study decks
(login-gated or paid); a browsable canon library beyond `/pavlov/list`.
