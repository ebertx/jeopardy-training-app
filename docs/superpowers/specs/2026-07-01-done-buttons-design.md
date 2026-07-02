# Done Buttons on Quiz Pages — Design Spec

**Date:** 2026-07-01
**Status:** Approved inline by user (option: all pages incl. Coryat in-game)

## Problem

Quiz-oriented pages have no consistent way back to the dashboard; the nav menu is the
only exit on Drill, Mastered, and Coryat, and Review's "End Session" only returns to the
in-page list.

## Design

A compact **Done** button, far right in each page header, that navigates to `/dashboard`.
Plain navigation only — no confirmation or summary ceremony, because all grades/answers
are persisted server-side at submit time (leaving loses nothing).

| Page | Change |
| --- | --- |
| `/drill` | Add **Done** to header (right-aligned). |
| `/mastered` | Add **Done** to header. |
| `/review` (list view) | Add **Done** to header; in-session End flow unchanged. |
| `/practice` | Make the existing **End** button always visible (already goes straight to dashboard when no session active; currently hidden until `sessionId` exists). |
| `/coryat` (landing) | Add **Done** to header. |
| `/coryat/[gameId]` (in-game) | Add **Exit** with same behavior; each answer is already persisted via `/api/coryat/{id}/answer`, so a mid-game exit leaves the game incomplete in history — no data loss. |

Styling: match the existing small header buttons (`px-3 py-1.5 rounded-lg border
border-gray-300 text-sm text-gray-700 hover:bg-gray-100`), adapted to dark headers where
needed. Verification: `npm run check` (0 errors) + `npm run build`.
