# History

## [Unreleased]

## [0.1.0] - 2026-11-11

Initial release.

### Added

- Path macros (`get!`, `call!`, `set!`, `update!`, `def!`, `del!`, `has!`, `own!`,
  `new!`, `wait!`, `is!`, `type_of!`, `keys!`, `entries!`) over `globalThis->` or
  `expr->` roots, supporting `.key` (stripping `r#`), `[key]`, `?.key`, `?.()`,
  intermediate calls, and `(..args)` spreads while rejecting `?.` in `set!`, `def!`,
  `update!`, and `new!` paths at compile time.
- Value and function builders (`obj!`, `arr!`, `val!`, `func!`, `proxy!`, `error!`):
  `obj!` and nested `val!` literals return `Value` for static keys or `Result<Value>`
  when any object uses a `..source` spread or `[computed]` key; `func!` wraps closures
  with `mut`, `once`, `async`, leading `this`, and `..rest` support; `proxy!` builds a
  `Proxy` with `get`, `set`, `has`, `define_property`, and `delete_property` traps.
- Value helpers and types (`Value`, `Result`, `ValueExt`, `List`, `ListItem`,
  `ListIter`, `Args`, `Type`, `string`, `dict`, `object`, `get_bound`, `serialize`,
  `deserialize`), including `ValueExt` readers (`present`, `text`, `expect_text`,
  `to_list`, `values`, `keys`, `entries`, `number`, `int`, `message`, `name`),
  JS-array-backed `List` and `ListIter` with on-demand `ListItem` conversion, variadic
  `Args`, `Type` (`typeof` inspection), and JSON-based `serialize` / `deserialize`.
- `Promise` and microtask helpers (`spawn`, `promise`, `resolve`, `reject`, `all`,
  `all_settled`) bridging Rust `Future`s and JS `Promise`s on the event loop.
- Optional `dom` feature: reference-counted `Observer` (`MutationObserver`) that
  disconnects on drop with `ObserveOptions` and `Mutation`, DOM helpers
  (`create_element`, `element`, `attach_shadow`, `csp_nonce`), and `web-sys`
  re-exports (`Document`, `Element`, `HtmlElement`, `ShadowRoot`, `Window`).
- Optional `event` feature: drop-cancelling `Timer` (`timeout`, `interval`, `frame`)
  and `Listener` (`new`, `of`) guards with `.detach()`, `ListenOptions`, timer futures
  (`sleep`, `timeout`), and event/frame futures and streams (`next_event`, `NextEvent`,
  `events`, `Events`, `frames`, `Frames`).
- Untrusted-input guarantees: object builders define own properties so `"__proto__"`
  never mutates prototypes; `..source` spreads and `[computed]` keys return `Err` on
  getter, `Proxy`, or coercion exceptions instead of unwinding through Rust; refused
  strict-mode writes return a `TypeError`; `List` keeps items in JS so large inputs do
  not exhaust the wasm `externref` table; and `deserialize` returns a `TypeError` on
  cyclic objects instead of overflowing the stack.
- Minimum supported Rust version (MSRV) 1.85 (`edition = "2024"`).
