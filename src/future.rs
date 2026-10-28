//! Bridging Rust `Future`s and JavaScript `Promise`s on the JS microtask queue.

use std::future::Future;

use crate::{Result, Value};
use js_sys::{Array, Promise};
use wasm_bindgen::JsCast as _;
use wasm_bindgen_futures::{JsFuture, future_to_promise};

/// Spawns a `'static` future onto the current thread's JS microtask queue.
///
/// The future runs in the background without returning a handle; use `promise` instead
/// when callers need to await completion or observe errors.
///
/// # Examples
///
/// ```no_run
/// js::spawn(async {
///     let _ = js::resolve(js::Value::from(1)).await;
/// });
/// ```
pub use wasm_bindgen_futures::spawn_local as spawn;

/// Converts a Rust `Future<Output = Result>` into a JavaScript `Promise` `Value`.
///
/// The returned `Promise` fulfills with `val` when the future outputs `Ok(val)` and
/// rejects with `err` when it outputs `Err(err)`.
///
/// # Examples
///
/// ```no_run
/// # async fn example() -> js::Result<()> {
/// let p = js::promise(async { Ok(js::Value::from("done")) });
/// assert_eq!(js::resolve(p).await?, "done");
///
/// let bad = js::promise(async { Err(js::error!("boom")) });
/// assert!(js::resolve(bad).await.is_err());
/// # Ok(())
/// # }
/// ```
pub fn promise(future: impl Future<Output = Result> + 'static) -> Value {
	future_to_promise(future).into()
}

/// Awaits a JavaScript `Promise` or normalizes a non-Promise `Value` via
/// `Promise.resolve`.
///
/// # Errors
///
/// Returns `Err(reason)` if `value` is a `Promise` (or thenable) that rejects.
///
/// # Examples
///
/// ```no_run
/// # async fn example() -> js::Result<()> {
/// assert_eq!(js::resolve(js::Value::from(7)).await?, 7);
/// assert!(js::resolve(js::reject("nope".into())).await.is_err());
/// # Ok(())
/// # }
/// ```
pub async fn resolve(value: Value) -> Result {
	JsFuture::from(Promise::resolve(&value)).await
}

/// Creates an immediately rejected JavaScript `Promise` with `error` as its rejection
/// reason (`Promise.reject(error)`).
///
/// # Examples
///
/// ```no_run
/// # async fn example() {
/// let p = js::reject(js::Value::from("failed"));
/// assert_eq!(js::resolve(p).await.unwrap_err(), "failed");
/// # }
/// ```
pub fn reject(error: Value) -> Value {
	Promise::reject(&error).into()
}

/// Awaits an iterator of `Promise`s or plain `Value`s concurrently with `Promise.all`,
/// returning their fulfilled values in input order.
///
/// # Errors
///
/// Fails fast and returns `Err(reason)` as soon as any input `Promise` rejects.
///
/// # Examples
///
/// ```no_run
/// use js::Value;
///
/// # async fn example() -> js::Result<()> {
/// let a = js::promise(async { Ok(Value::from(1)) });
/// let b = Value::from(2);
/// assert_eq!(js::all([a, b]).await?, vec![Value::from(1), Value::from(2)]);
///
/// assert!(js::all([js::reject("err".into())]).await.is_err());
/// # Ok(())
/// # }
/// ```
pub async fn all(values: impl IntoIterator<Item = Value>) -> Result<Vec<Value>> {
	settle(Promise::all(&values.into_iter().collect::<Array>())).await
}

/// Awaits an iterator of `Promise`s or plain `Value`s via `Promise.allSettled`,
/// returning each outcome in input order as `Ok(value)` or `Err(reason)`.
///
/// Never short-circuits on rejection: every item settles before the returned future
/// completes.
///
/// # Examples
///
/// ```no_run
/// use js::Value;
///
/// # async fn example() {
/// let outcomes = js::all_settled([Value::from(1), js::reject("no".into())]).await;
/// assert_eq!(outcomes, vec![Ok(Value::from(1)), Err(Value::from("no"))]);
/// # }
/// ```
pub async fn all_settled(values: impl IntoIterator<Item = Value>) -> Vec<Result> {
	let values = values.into_iter().collect::<Array>();
	let Ok(results) = settle(Promise::all_settled(&values)).await else {
		return Vec::new();
	};
	// Each element of `Promise.allSettled` is `{ status: "fulfilled", value }` or
	// `{ status: "rejected", reason }`.
	results
		.iter()
		.map(|result| {
			let fulfilled = crate::get!(result->status).is_ok_and(|s| s == "fulfilled");
			if fulfilled {
				crate::get!(result->value)
			} else {
				Err(crate::get!(result->reason).unwrap_or(Value::UNDEFINED))
			}
		})
		.collect()
}

// Awaits a `Promise` that resolves to a JS `Array` and converts it to `Vec<Value>`.
async fn settle(promise: Promise) -> Result<Vec<Value>> {
	let results = JsFuture::from(promise).await?;
	Ok(results.unchecked_into::<Array>().to_vec())
}
