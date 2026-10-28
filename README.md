# pka-webarrow

Macros and helpers for crossing the JS boundary from Rust compiled to `wasm32`. Paths
after `->` follow JS syntax:

```rust
let body = js::get!(globalThis->document.body)?;
js::set!(body->style.color = "red")?;
js::call!(body->setAttribute("aria-busy", true))?;

let init = js::obj! { method: "POST", headers: js::obj! { "x-trace": "on" } };
let response = js::wait!(globalThis->fetch("/api", init)).await?;
let data = js::wait!(response->json()).await?;
let name = js::get!(data->user?.name)?; // `undefined` if `user` is missing
```

Each step that can throw returns a `Result` with the thrown JS value as its `Err`.

## Install

The library is named `js`, so the examples work without renaming:

```toml
[dependencies]
pka-webarrow = { version = "0.1", features = ["dom", "event"] }
```

| Feature | Adds |
|---|---|
| `dom` | `Observer` (`MutationObserver`), element helpers |
| `event` | `Timer` (`setTimeout`, `setInterval`, `requestAnimationFrame`), `Listener` (`addEventListener`), `sleep`, `timeout`, `next_event`, `events`, `frames` |

Both are off by default. Without them, `web-sys` and the timer dependencies are not
built.

## Macros

| Macro | JS |
|---|---|
| `get!(a->b.c)` | `a.b.c` |
| `call!(a->b.c(x))` | `a.b.c(x)` |
| `set!(a->b = x)` | `a.b = x` |
| `update!(a->b, \|old\| ..)` | `a.b = f(a.b)` |
| `def!(a->b, { get: f })` | `Object.defineProperty(a, "b", { get: f })` |
| `del!(a->b)` | `delete a.b` |
| `has!(a->b)` / `own!(a->b)` | `"b" in a` / `Object.hasOwn(a, "b")` |
| `new!(globalThis->Map())` | `new Map()` |
| `wait!(a->b())` | `await a.b()` |
| `is!(a, HTMLElement)` | `a instanceof HTMLElement` |
| `type_of!(a->b)` | `typeof a.b` |
| `keys!(a->b)` / `entries!(a->b)` | `Object.keys(a.b)` / `Object.entries(a.b)` |

Paths support `.key`, `[key]`, `?.key`, `?.()`, calls in any step, and `(..args)`
spread. Syntax that JS rejects, such as `?.` on the left of an assignment, fails to
compile.

## Values

- `obj! { id: 1, "aria-label": label, [key]: value, ..defaults }?` builds an object, and
  `arr![1, "two", ..rest]` an array. `obj!` returns a plain `Value` when every key is an
  identifier or literal, or `js::Result<Value>` when it includes `[key]` or `..source`.
- `val!({ id: 1, tags: ["a", ..extra], parent: null })` builds nested objects and
  arrays from one literal (returning `js::Result<Value>` if any nested object uses
  `[key]` or `..source`).
- `func!(|a, b| ..)` creates a JS function from a closure. It supports `mut`, `once`,
  `async`, a leading `this` parameter (alone or with `mut`, `once`, or `async`), and
  `..rest`.
- `proxy!(target, { get: |target, key| .. })` creates a `Proxy`.
- `error!("bad {name}")` builds an `Error`.
- `ValueExt` reads values with `text`, `number`, `to_list`, `message`, and more.
- `serialize` and `deserialize` convert between Rust and JS values through JSON.

### `obj!` is not `serde_json::json!`

Identifier keys in `obj!` are names, like JS. In `json!` they are variables:

```rust
let a = "key";
js::obj! { a: 1 }            // { a: 1 }
serde_json::json!({ a: 1 })  // { "key": 1 }
js::obj! { [a]: 1 }?         // { key: 1 }, a computed key as in JS
```

## Untrusted input

Values may come from pages you don't control:

- Builders define keys instead of assigning them, so a `"__proto__"` key (or spread)
  never changes a prototype.
- Object spreads (`..source`) and computed keys (`[key]`) in `obj!` and `val!` catch
  exceptions thrown by page getters, `Proxy` traps, or `toString` / `valueOf` /
  `Symbol.toPrimitive` and return them as `Err` instead of unwinding through Rust
  frames.
- Writes that strict-mode JS refuses, such as assigning to a frozen object, are a
  `TypeError` instead of failing silently.
- `deserialize` returns a `TypeError` for a cyclic object instead of overflowing the
  stack.

## Development

Tests run in a headless browser through `wasm-bindgen-test-runner`, whose version must
match `wasm-bindgen` in `Cargo.lock`:

```sh
cargo install wasm-bindgen-cli --version <wasm-bindgen version in Cargo.lock>
cargo xtask ci
```

`cargo xtask ci` runs the same steps as CI: a formatting check, clippy and rustdoc with
warnings denied, a compile check of the doc examples, an MSRV check, and the browser
tests. `cargo xtask fmt`, `lint`, `doc`, `msrv`, and `test` run one step each.

`cargo xtask msrv` invokes `rustup run` with the toolchain from `rust-version` in
`Cargo.toml` (currently 1.85) to check every target and feature. That toolchain must be
installed with the `wasm32-unknown-unknown` target.

## License

BSD-3-Clause. See [LICENSE.md](LICENSE.md).
