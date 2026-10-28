//! Browser integration tests for JSON-based `serialize` and `deserialize` helpers.

#![cfg(target_arch = "wasm32")]

use std::collections::BTreeMap;

use js::{Value, ValueExt as _};
use serde::{Deserialize, Serialize};
use serde_json::json;
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

wasm_bindgen_test_configure!(run_in_browser);

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Settings {
	name: String,
	count: u32,
	tags: Vec<String>,
	theme: Option<String>,
}

#[wasm_bindgen_test]
fn deserialize_reads_json_shapes() {
	let value = js::obj! { a: 1, b: js::arr![true, Value::NULL], c: "s" };
	let read: serde_json::Value = js::deserialize(&value).unwrap();
	assert_eq!(read, json!({ "a": 1, "b": [true, null], "c": "s" }));
	let number: serde_json::Value = js::deserialize(&Value::from(1.5)).unwrap();
	assert_eq!(number, json!(1.5));
}

#[wasm_bindgen_test]
fn deserialize_reads_typed_structs() {
	let value = js::val!({ name: "pod", count: 3, tags: ["a", "b"] });
	let settings: Settings = js::deserialize(&value).unwrap();
	assert_eq!(
		settings,
		Settings {
			name: "pod".into(),
			count: 3,
			tags: vec!["a".into(), "b".into()],
			theme: None,
		}
	);
	let wrong = js::val!({ name: 1, count: 3, tags: [] });
	let error = js::deserialize::<Settings>(&wrong).unwrap_err();
	assert!(error.message().contains("invalid type"), "{}", error.message());
}

// `deserialize` delegates to `JSON.stringify`, so `Date.toJSON()` runs, `undefined` /
// functions / symbols are omitted from objects or turned into `null` in arrays, and
// top-level `undefined` or functions deserialize as `null` (`None` for `Option<T>`).
#[wasm_bindgen_test]
fn deserialize_follows_json_stringify() {
	let date = js::new!(globalThis->Date(0)).unwrap();
	let symbol = js::call!(globalThis->Symbol()).unwrap();
	let value = js::val!({
		date: date,
		gone: undefined,
		call: js::func!(|| 1),
		symbol: symbol,
		list: [undefined, 1],
	});
	let read: serde_json::Value = js::deserialize(&value).unwrap();
	assert_eq!(read, json!({ "date": "1970-01-01T00:00:00.000Z", "list": [null, 1] }));

	assert_eq!(js::deserialize::<Option<u8>>(&Value::UNDEFINED).unwrap(), None);
	assert_eq!(js::deserialize::<Option<u8>>(&js::func!(|| 1)).unwrap(), None);
	let null: serde_json::Value = js::deserialize(&Value::UNDEFINED).unwrap();
	assert_eq!(null, json!(null));
}

// Cyclic objects and `BigInt` values must return a `TypeError` from `JSON.stringify`
// rather than overflowing the wasm stack or trapping.
#[wasm_bindgen_test]
fn deserialize_reports_cycles_and_bigints() {
	let cyclic = js::obj! {};
	js::set!(cyclic->me = cyclic).unwrap();
	let error = js::deserialize::<serde_json::Value>(&cyclic).unwrap_err();
	assert!(js::is!(error, TypeError));
	let big = js::call!(globalThis->BigInt(1)).unwrap();
	assert!(js::deserialize::<serde_json::Value>(&big).is_err());
}

#[wasm_bindgen_test]
fn serialize_round_trips() {
	let source = json!({ "text": "a\"b\n", "list": [1, { "x": null }] });
	let value = js::serialize(&source).unwrap();
	assert_eq!(js::get!(value->text).unwrap(), "a\"b\n");
	assert!(js::get!(value->list[1].x).unwrap().is_null());
	assert_eq!(js::deserialize::<serde_json::Value>(&value).unwrap(), source);

	let settings = Settings { name: "pod".into(), count: 1, tags: vec![], theme: None };
	let value = js::serialize(&settings).unwrap();
	assert!(js::get!(value->theme).unwrap().is_null());
	assert_eq!(js::deserialize::<Settings>(&value).unwrap(), settings);

	let map = BTreeMap::from([("k", 1)]);
	let value = js::serialize(&map).unwrap();
	assert!(!js::is!(value, Map));
	assert_eq!(js::get!(value->k).unwrap(), 1);
}

// Pins the documented limits of `serialize`: integers above `2^53 - 1` lose precision
// when parsed into JS `Number` (`f64`), and maps with non-string keys fail.
#[wasm_bindgen_test]
fn serialize_reports_its_limits() {
	let big = js::serialize(&u64::MAX).unwrap();
	assert_eq!(big.as_f64().unwrap(), u64::MAX as f64);
	assert_ne!(js::string(&big).unwrap(), u64::MAX.to_string());
	let tuple_keys = BTreeMap::from([((1, 2), "x")]);
	assert!(js::serialize(&tuple_keys).is_err());
}
