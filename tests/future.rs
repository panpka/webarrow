//! Browser integration tests for `spawn`, `promise`, `resolve`, `reject`, `all`, and
//! `all_settled`.

#![cfg(target_arch = "wasm32")]

mod common;

use common::{counter, sleep};
use js::Value;
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
async fn spawn_runs_future_in_background() {
	let (runs, finish) = counter();
	js::spawn(async move {
		sleep(0).await;
		finish();
	});
	assert_eq!(runs.get(), 0);
	sleep(20).await;
	assert_eq!(runs.get(), 1);
}

#[wasm_bindgen_test]
async fn promise_wraps_future_result() {
	let fulfilled = js::promise(async { Ok("done".into()) });
	assert!(js::is!(fulfilled, Promise));
	assert_eq!(js::resolve(fulfilled).await.unwrap(), "done");

	let rejected = js::promise(async { Err("fail".into()) });
	assert!(js::is!(rejected, Promise));
	assert_eq!(js::resolve(rejected).await.unwrap_err(), "fail");
}

// `resolve` awaits `Promise`s and passes non-Promise values through via
// `Promise.resolve`.
#[wasm_bindgen_test]
async fn resolve_awaits_promises_and_passes_values() {
	let promise = js::call!(globalThis->Promise.resolve(7)).unwrap();
	assert_eq!(js::resolve(promise).await.unwrap(), 7);
	let rejected = js::call!(globalThis->Promise.reject("no")).unwrap();
	assert_eq!(js::resolve(rejected).await.unwrap_err(), "no");
	assert_eq!(js::resolve("plain".into()).await.unwrap(), "plain");
	assert!(js::resolve(Value::UNDEFINED).await.unwrap().is_undefined());
}

#[wasm_bindgen_test]
async fn reject_returns_rejected_promise() {
	let rejected = js::reject("no".into());
	assert!(js::is!(rejected, Promise));
	assert_eq!(js::resolve(rejected).await.unwrap_err(), "no");
}

#[wasm_bindgen_test]
async fn all_keeps_input_order_and_fails_fast() {
	let slow = js::promise(async {
		sleep(20).await;
		Ok("slow".into())
	});
	let fast = js::call!(globalThis->Promise.resolve("fast")).unwrap();
	let results = js::all([slow, fast, Value::from(3)]).await.unwrap();
	assert_eq!(results, vec![Value::from("slow"), Value::from("fast"), Value::from(3)]);

	let rejected = js::reject("x".into());
	assert_eq!(js::all([rejected, Value::from(1)]).await.unwrap_err(), "x");
}

#[wasm_bindgen_test]
async fn all_settled_splits_fulfilled_and_rejected() {
	let fulfilled = js::call!(globalThis->Promise.resolve(1)).unwrap();
	let rejected = js::reject("x".into());
	let results = js::all_settled([fulfilled, rejected, Value::from(3)]).await;
	assert_eq!(
		results,
		vec![Ok(Value::from(1)), Err(Value::from("x")), Ok(Value::from(3))]
	);
	assert!(js::all_settled([]).await.is_empty());
}
