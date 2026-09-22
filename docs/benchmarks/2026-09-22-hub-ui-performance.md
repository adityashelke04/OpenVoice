# Hub UI performance, 2026-09-22

What the Home screen cost before and after the change that stopped it building
two hundred rows in order to show about fifteen.

## Method

Release build of `ov-app` with the frontend embedded
(`tauri build --no-bundle`), launched with

```
WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333
```

and driven over the DevTools protocol through `scripts/cdp.mjs`, attaching to
the target whose url contains `window=hub`. This is the only way to measure the
Hub as it actually ships: the pixel twin renders it in Chrome with a stubbed
bridge and `data-material="none"`, so it cannot reach the real window's state
(see `DESIGN.md` §14 for the bug that blind spot once hid).

Numbers come from `Performance.getMetrics`, `performance.getEntriesByType` and
direct DOM queries. The machine's own history was the data: 643 dictations,
Home on screen, default 1100×740 window, Glacier dark.

Counts are exact. Durations are from a single clean load and will vary by a few
per cent between runs; the ratios are what matter, and they are large enough
that run-to-run noise does not explain them.

## Results

| Measurement | Before | After | Change |
|---|---|---|---|
| `.rows` children in the DOM | 208 | 18 | −91% |
| …`hidden` | 193 | 10 | |
| …visible | 15 | 8 | |
| DOM nodes under `.rows` | 2,403 | 196 | −92% |
| Icon SVGs under `.rows` | 398 | 32 | −92% |
| Buttons under `.rows` | 398 | 32 | −92% |
| Whole-document elements | 2,576 | 371 | −86% |
| Document nodes (incl. text) | 6,556 | 1,082 | −83% |
| Layouts, clean load + 6 s idle | 781 | 15 | −98% |
| Layout time, same | 421 ms | 72 ms | −83% |
| Recalc-style count, same | 796 | 26 | −97% |
| Recalc-style time, same | 116 ms | 30 ms | −74% |
| Total task time, same | 787 ms | 308 ms | −61% |
| Returning to Home | 390 layouts, 184.3 ms | 7 layouts, 14.3 ms | −96% layout time |
| One shrink+restore of the list box | 784 layouts, 320 ms | 14 layouts, 13 ms | −96% |
| JS heap after load | 9.2 MB | 5.7 MB | −38% |
| Hub JS chunk | 327.1 kB | 268.6 kB | −18% |
| First contentful paint | 436 ms | 432 ms | unchanged |

The shrink-and-restore row is the one a person feels: that sequence is what a
single step of a window-resize drag costs, and it ran on every frame.

First contentful paint did not move, and was not expected to — it is dominated
by font loading and the engine's own startup, neither of which this touched.
Reporting it because a performance note that only lists what improved is not a
measurement, it is an advertisement.

## Where the time went

Three independent costs, all consequences of rendering rows nobody could see.

**Layout thrashing.** `fitChildren` read an element's rectangle after each write
to `hidden`, so the browser re-ran layout once per row. Over 208 children that
was 114.9 ms against 4.33 ms with the reads batched — measured by running both
forms back to back in the live page. Batching also matches what the function's
own header already described.

**Regex compilation.** `wholeWord` built a fresh `RegExp` on every call:
twenty-four per row, about 4,800 per render of the list. 19.3 ms against 0.99 ms
once compiled patterns are kept.

**Unmemoized rows.** Anything changing on Home re-ran `dictionaryHits` and two
icon components for every mounted row. Memoizing `Row` on its own changed
nothing, because four separate things broke the shallow compare: a new
`onToggle` closure per row, a `patch` rebuilt on every render of `useSettings`,
`settings?.dictionary ?? []` allocating a fresh array, and a `now` prop `Row`
never read that changed every minute.

On top of those, `now` sat in `useFit`'s dependency list, so once a minute the
whole list re-measured for nothing.

## Measured and deliberately left alone

- **The four `blur(90px)` ambient blobs.** Static: painted once, not
  re-rasterised while idle. `DESIGN.md` §14 records that hiding them under Mica
  was a shipped bug.
- **`backdrop-filter`.** Zero elements use it. Nothing to fix.
- **Fonts.** 511 kB ships, but only four files totalling 169 kB are ever
  fetched — `@fontsource`'s `unicode-range` subsetting works. The remainder
  (Borel's math, symbols and Vietnamese subsets, the `.woff` fallbacks Chromium
  never requests, Cyrillic) is installer size, not load time. Worth a separate
  cleanup; it would not change a single number above.
