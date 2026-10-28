//! Macros and helpers for crossing the JavaScript boundary from `wasm32` Rust.
//!
//! Most operations in this crate take a JavaScript-like path starting with `->`. The
//! expression before `->` is the root (either `globalThis` or any Rust expression that
//! implements `AsRef<JsValue>`), and the steps after `->` follow JavaScript property,
//! indexing, optional-chaining, and call syntax:
//!
//! ```no_run
//! # async fn example() -> js::Result<()> {
//! let body = js::get!(globalThis->document.body)?;
//! js::set!(body->style.color = "red")?;
//! js::call!(body->setAttribute("aria-busy", true))?;
//!
//! let init = js::obj! { method: "POST", headers: js::obj! { "x-trace": "on" } };
//! let response = js::wait!(globalThis->fetch("/api", init)).await?;
//! let data = js::wait!(response->json()).await?;
//! let name = js::get!(data->user?.name)?;
//! # let _ = name;
//! # Ok(())
//! # }
//! ```
//!
//! # Macro families
//!
//! - **Path macros** walk a JS path and perform an operation on the target:
//!   `get!`, `call!`, `set!`, `update!`, `def!`, `del!`, `has!`, `own!`, `new!`,
//!   `wait!`, `is!`, `type_of!`, `keys!`, and `entries!`.
//! - **Value builders** construct JS values and functions from Rust literals and
//!   closures: `obj!`, `arr!`, `val!`, `func!`, `proxy!`, and `error!`.
//! - **Value and Promise helpers** (`ValueExt`, `List`, `Args`, `Type`, `serialize`,
//!   `deserialize`, `spawn`, `promise`, `resolve`, `reject`, `all`, `all_settled`)
//!   handle conversions and asynchronous execution on the JS event loop.
//!
//! # Cargo features
//!
//! - `dom`: enables `web-sys` DOM element helpers and `Observer` (`MutationObserver`).
//! - `event`: enables `Timer`, `Listener`, `sleep`, `timeout`, `frames`, `next_event`,
//!   and `events`.
//!
//! Every public item enabled by a feature is re-exported at the crate root.

#![warn(missing_docs)]

// Macro expansion helpers and inline JS shims; hidden from user docs but public so
// exported macros can call into `$crate::r#macro::*` from downstream crates.
#[doc(hidden)]
pub mod r#macro;

mod future;
mod value;

#[cfg(feature = "dom")]
mod dom;
#[cfg(feature = "event")]
mod event;

#[cfg(feature = "dom")]
pub use dom::*;
#[cfg(feature = "event")]
pub use event::*;
pub use future::*;
pub use value::*;

// Re-exported for macro expansions that mention `$crate::wasm_bindgen::JsValue`.
#[doc(hidden)]
pub use wasm_bindgen;

pub(crate) use value::Result as JsResult;
