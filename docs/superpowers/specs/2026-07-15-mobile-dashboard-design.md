# Mobile (portrait) dashboard optimization — design

Date: 2026-07-15
Status: approved (user picked "mobile-tailored layout" over a light responsive pass)

## Problem

The dashboard at portrait-phone width (~390px) has real breakage, verified by
screenshot with mock data:

- **Focus Areas rows overflow**: fixed columns (label `w-52`, share `w-10`,
  stats `w-32`, all `shrink-0`) exceed the viewport, squeezing the bar to zero
  width, clipping the stats column off-screen, and causing horizontal page
  scroll.
- **Category Performance** is a vertical bar chart with 13 rotated x-axis
  labels in ~350px — unreadable.
- **Category Breakdown table** wraps category names over 2–3 lines and needs
  sideways scrolling to reach the Review column.
- The 14-day forecast chart labels are cramped; the Practice button floats in
  leftover wrap space.

## Approach

All changes in `frontend/src/routes/dashboard/+page.svelte`. Phone = below
Tailwind's `sm` breakpoint (640px). Desktop rendering stays pixel-identical.

1. **Focus Areas** — two-line rows on phones via Tailwind `order`/width
   utilities (no JS): line 1 = category name + "N% right · M tries"; line 2 =
   full-width bar + share %. The bar and share % are grouped in a sub-flex div
   that is `w-full` below `sm` and `flex-1` at `sm+`, preserving the current
   desktop row layout exactly.
2. **Category Performance chart** — below `sm`, switch Chart.js to
   `indexAxis: 'y'` (horizontal bars, category names as y-axis labels), hide
   the legend, and grow the card height with the category count
   (~28px/row + axis padding). Breakpoint detection via
   `<svelte:window bind:innerWidth />` + `$derived` — SSR sees width 0 →
   desktop config, corrected on hydration (charts are client-only anyway).
3. **Forecast** — 7 days below `sm`, 14 at `sm+` (same `innerWidth` state).
4. **Category Breakdown table** — Total and Correct columns get
   `hidden sm:table-cell`; the Cold/Review cells already show counts in
   parentheses, so no information is lost on phones.
5. **Practice button** — `w-full text-center` below `sm`, current
   `ml-auto` placement at `sm+`.

## Testing

- `svelte-check` clean.
- Visual verification against the mock API (`scratchpad/mock-api.js`) via
  Playwright at 390×844 (every reworked section) and at 1280px wide to confirm
  desktop is unchanged.
