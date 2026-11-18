# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

(Desktop Windows 11 app; the browser chrome is HTML/CSS rendered in a WebView2 inside a native Rust window.)

## Stack

Rust (tao + wry) shell over the system WebView2 runtime; chrome UI is one dependency-free HTML/CSS/JS file. Chosen by the user: WebView2 engine, Rust.

## Users

The owner (French-speaking power user) browsing daily with many tabs open, often 50+, who wants Brave's privacy and ergonomics without its memory cost.

## Product Purpose

Leirk is a lightweight desktop browser: Brave's ergonomics and Shields, a few Vivaldi power features, minimal RAM. Success = 50 open tabs cost about what a handful does, and nothing in the chrome feels slower or clumsier than Brave or Safari.

## Positioning

Tabs are data, not processes: a tab only owns a WebView2 while visible or recently used; the rest are suspended, then unloaded. New tabs cost nothing until navigation.

## Capabilities and Constraints

- Brave: Shields (adblock-rust with EasyList, EasyPrivacy, uBO, Liste FR; per-site toggle, blocked counter), private tabs (InPrivate).
- Vivaldi: tab stacks, tiling 2-4 tabs, command palette (F2).
- Every search engine reachable by keyword or `!bang`; default engine selectable; engines imported from Chromium browsers.
- Import bookmarks and history from all installed Chromium and Firefox-family browsers. Passwords not imported (app-bound encryption).
- Out of scope v1: Rewards/BAT, Wallet, Leo, Tor, sync, extensions.
- UI language: French.

## Brand Commitments

Name: Leirk. Direction confirmed by the user: a mix of Brave's ergonomics (Shields next to the address, tabs above toolbar, ☰ menu) and Apple/Safari finish (calm, compact, translucent).

## Product Principles

1. Memory is a feature: every UI decision must not cost a process.
2. Known patterns first: Brave/Safari muscle memory works unchanged.
3. Keyboard-complete: every action reachable from F2 and standard shortcuts.
4. Quiet chrome, page first.
