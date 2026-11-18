# TDD evidence: Leirk v0.1

Source plan: none; journeys derived during this run from the user's answers.

## Journeys
1. As a heavy tab user, I keep 50 tabs open while only a few cost memory.
2. As a Brave user, ads and trackers are blocked, with a per-site off switch.
3. As anyone, I type a URL, a search, or `kw query` / `!kw query` and land in the right place.
4. As a switcher, I import bookmarks, history and engines from my other browsers.

## RED → GREEN
| Stage | Command | Result | Commit |
|---|---|---|---|
| RED | `cargo test --lib` | 1 passed, 23 failed (all on `todo!()`) | 2276ab6 |
| GREEN | `cargo test --lib` | 24 passed (one test expectation fixed: `!` encodes to `%21`) | 8085091 |
| App | `cargo test` | 24 lib + 2 bin passed | 5a47588 |

## Guarantees
| # | Guarantee | Test |
|---|---|---|
| 1 | Unload picks idle or least recently used tabs beyond the budget; never visible or audible tabs | `tabs::unload_policy_respects_idle_lru_visible_and_audio` |
| 2 | Closing the active tab selects its right neighbour, then the left; the last close leaves a blank tab | `tabs::close_active_picks_right_then_left`, `closing_last_tab_leaves_a_blank_one` |
| 3 | Stacks group, reorder and dissolve at one member; tiling shows 2-4 tabs only while one is active | `tabs::stack_*`, `tiling_*`, `tile_rects_layouts` |
| 4 | URLs pass through or get https/http; text goes to the default engine; keywords and bangs pick engines; UTF-8 is percent-encoded | `omnibox::*` |
| 5 | Blocking honours exceptions, `$third-party`, per-site disable; cosmetic selectors are site-scoped | `shields::*` |
| 6 | History ranks by visits, matches all words, escapes LIKE wildcards, keeps known titles, merges imports | `store::*` |
| 7 | Chromium bookmarks flatten with folder paths and drop `javascript:`; Google-templated engines are rejected | `import::*` |
| 8 | Shortcut map and base64 encoder | `main::tests` |

## Gaps
Window, WebView2 hooks and UI were verified by running the release build with scripted clicks and keys and checking screenshots (new tab page, navigation, palette, 12-tab restore), not by automated E2E. Coverage tooling is not installed (`cargo llvm-cov` absent), so no coverage percentage was measured.
