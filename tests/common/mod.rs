//! Shared test fixtures and instrumentation helpers for browser integration tests.

#![allow(dead_code)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use web_sys::Document;

/// Returns the active browser `Document` from `window.document`.
pub fn document() -> Document {
	web_sys::window().unwrap().document().unwrap()
}

/// Builds a JS function that always throws an `Error` with `message`.
pub fn thrower(message: &str) -> js::Value {
	let message = message.to_owned();
	js::func!(|| -> js::Result { Err(js::error!("{message}")) })
}

/// Waits for `ms` milliseconds using a raw `Promise` + `setTimeout` (independent of
/// the `event` feature's `sleep` helper).
pub async fn sleep(ms: i32) {
	let executor = js::func!(once |resolve| -> js::Result {
		js::call!(globalThis->setTimeout(resolve, ms))
	});
	let promise = js::new!(globalThis->Promise(executor)).unwrap();
	js::resolve(promise).await.unwrap();
}

/// Shared append-only log used by tests to record callback arguments in order.
pub type Log<T> = Rc<RefCell<Vec<T>>>;

/// Creates a `(Log<T>, push_fn)` pair for recording values from closures.
pub fn log<T: 'static>() -> (Log<T>, impl Fn(T) + Clone + 'static) {
	let log = Log::default();
	let sink = log.clone();
	(log, move |value| sink.borrow_mut().push(value))
}

/// Creates a shared call counter and an increment closure.
pub fn counter() -> (Rc<Cell<u32>>, impl Fn() + Clone + 'static) {
	let count = Rc::new(Cell::new(0));
	let shared = count.clone();
	(count, move || shared.set(shared.get() + 1))
}

/// Drop tracker that reports whether any clone moved into a closure is still alive.
#[derive(Clone, Debug, Default)]
pub struct Probe(Rc<()>);

impl Probe {
	/// Returns `true` while at least one clone besides `self` has not been dropped.
	pub fn held(&self) -> bool {
		Rc::strong_count(&self.0) > 1
	}
}
