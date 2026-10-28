//! Browser integration tests for `ValueExt`, `List`, `Args`, `string`, `dict`,
//! `object`, and `get_bound`.

#![cfg(target_arch = "wasm32")]

mod common;

use common::{document, thrower};
use js::{Args, Value, ValueExt as _};
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

wasm_bindgen_test_configure!(run_in_browser);

// `present()` only filters `null` and `undefined`; falsy values (`0`, `false`, `""`)
// remain `Some`.
#[wasm_bindgen_test]
fn present_filters_only_nullish() {
	assert!(Value::NULL.present().is_none());
	assert!(Value::UNDEFINED.present().is_none());
	assert_eq!(Value::from(0).present().unwrap(), 0);
	assert_eq!(Value::from(false).present().unwrap(), false);
	assert_eq!(Value::from("").present().unwrap(), "");
}

#[wasm_bindgen_test]
fn text_returns_strings_without_converting() {
	assert_eq!(Value::from("abc").text(), "abc");
	assert_eq!(Value::from(12).text(), "");
	assert_eq!(Value::NULL.text(), "");
	assert_eq!(js::obj! {}.text(), "");
}

#[wasm_bindgen_test]
fn expect_text_errors_with_name() {
	assert_eq!(Value::from("ok").expect_text("name").unwrap(), "ok");
	let error = Value::from(1).expect_text("name").unwrap_err();
	assert!(js::is!(error, Error));
	assert_eq!(error.message(), "name must be a string");
	assert!(Value::UNDEFINED.expect_text("name").is_err());
}

#[wasm_bindgen_test]
fn to_list_reads_length_and_indices() {
	let document = document();
	let nodes = js::call!(document->querySelectorAll("body")).unwrap();
	let body = nodes.to_list().unwrap();
	assert_eq!(body.len(), 1);
	assert_eq!(body.get(0).unwrap(), Value::from(document.body().unwrap()));
	assert!(body.get(1).is_none());

	let fake = js::obj! { length: 2, "0": "a", "1": "b" };
	let values = fake.to_list().unwrap().to_vec();
	assert_eq!(values, vec![Value::from("a"), Value::from("b")]);

	assert!(js::obj! { "0": "a" }.to_list().unwrap().is_empty());
	assert!(js::arr![].to_list().unwrap().is_empty());
	assert!(Value::NULL.to_list().is_err());
}

// `to_list` delegates to `Array.prototype.slice.call`, coercing `length` via JS
// `ToLength` and reading sparse holes as `undefined`.
#[wasm_bindgen_test]
fn to_list_converts_length_like_js() {
	for length in [f64::NAN, -1.0, f64::NEG_INFINITY] {
		assert!(js::obj! { length: length }.to_list().unwrap().is_empty(), "{length}");
	}
	let fake = js::obj! { length: 1.9, "0": "a" };
	assert_eq!(fake.to_list().unwrap().to_vec(), vec![Value::from("a")]);
	let fake = js::obj! { length: "2", "0": "a", "1": "b" };
	assert_eq!(fake.to_list().unwrap().len(), 2);
	let holes = js::obj! { length: 2, "1": "b" }.to_list().unwrap();
	assert!(holes.get(0).unwrap().is_undefined());
}

// Lengths above `2^32 - 1` (`4_294_967_295`) exceed the maximum JS `Array` length and
// fail with a `RangeError` from `Array.prototype.slice`.
#[wasm_bindgen_test]
fn to_list_rejects_lengths_js_rejects() {
	for length in [f64::INFINITY, 4_294_967_296.0, 1e300] {
		let error = js::obj! { length: length }.to_list().unwrap_err();
		assert_eq!(error.name(), "RangeError", "{length}");
	}
}

// A `List` holds a single JS `Array` handle rather than one `externref` table slot per
// element, so lists larger than the browser's 10M `externref` limit (~8.4M live
// `JsValue`s) do not abort the wasm instance.
#[wasm_bindgen_test]
fn to_list_keeps_huge_lists_in_js() {
	let length = 10_000_000u32;
	let list = js::obj! { length: length }.to_list().unwrap();
	assert_eq!(list.len(), length);
	assert!(list.get(length - 1).unwrap().is_undefined());
	assert_eq!(list.iter().take(3).count(), 3);
}

#[wasm_bindgen_test]
fn values_reads_any_iterable() {
	let set = js::new!(globalThis->Set(js::arr![1, 2])).unwrap();
	assert!(set.to_list().unwrap().is_empty());
	assert_eq!(set.values().unwrap().to_vec(), vec![Value::from(1), Value::from(2)]);

	let map = js::new!(globalThis->Map(js::arr![js::arr!["k", "v"]])).unwrap();
	let pair = map.values().unwrap().get(0).unwrap();
	assert_eq!(js::get!(pair->[0]).unwrap(), "k");
	assert_eq!(js::get!(pair->[1]).unwrap(), "v");

	let error = js::obj! {}.values().unwrap_err();
	assert_eq!(error.name(), "TypeError");
	assert!(Value::from(1).values().is_err());
	assert!(Value::NULL.values().is_err());
}

#[wasm_bindgen_test]
fn list_iterates_like_a_vec() {
	let list = js::arr![1, 2, 3].to_list().unwrap();
	let mut iter = list.iter();
	assert_eq!(iter.len(), 3);
	assert_eq!(iter.next_back().unwrap(), 3);
	assert_eq!(iter.next().unwrap(), 1);
	assert_eq!(iter.len(), 1);
	assert_eq!(iter.next().unwrap(), 2);
	assert!(iter.next().is_none());

	let mut sum = 0.0;
	for item in &list {
		sum += item.as_f64().unwrap();
	}
	assert_eq!(sum, 6.0);
	let doubled: Vec<f64> =
		list.clone().into_iter().map(|v| v.as_f64().unwrap() * 2.0).collect();
	assert_eq!(doubled, [2.0, 4.0, 6.0]);
	assert_eq!(Value::from(list.clone()), Value::from(list.clone()));
}

#[wasm_bindgen_test]
fn list_converts_to_fixed_arrays_by_length() {
	let [a, b] = js::arr!["a", "b"].to_list().unwrap().try_into().unwrap();
	assert_eq!((a, b), (Value::from("a"), Value::from("b")));
	let result: Result<[Value; 3], Value> = js::arr![1].to_list().unwrap().try_into();
	let error = result.unwrap_err();
	assert_eq!(error.name(), "RangeError");
	assert_eq!(error.message(), "expected 3 items, got 1");
}

#[wasm_bindgen_test]
fn entries_reads_own_enumerable_pairs() {
	let parent = js::obj! { inherited: 0 };
	let object = js::call!(globalThis->Object.create(parent)).unwrap();
	js::set!(object->a = 1).unwrap();
	js::set!(object->b = "x").unwrap();
	let entries = object.entries().unwrap().to_vec();
	assert_eq!(
		entries,
		vec![("a".to_owned(), Value::from(1)), ("b".to_owned(), Value::from("x"))]
	);
	assert!(Value::NULL.entries().is_err());
}

#[wasm_bindgen_test]
fn keys_reads_own_enumerable_string_keys() {
	let parent = js::obj! { inherited: 0 };
	let object = js::call!(globalThis->Object.create(parent)).unwrap();
	js::set!(object->b = 1).unwrap();
	js::set!(object->a = 2).unwrap();
	let symbol = js::call!(globalThis->Symbol()).unwrap();
	js::set!(object->[symbol] = 3).unwrap();
	assert_eq!(object.keys().unwrap(), ["b", "a"]);
	assert!(Value::UNDEFINED.keys().is_err());
}

// Pins the documented UTF-16 -> UTF-8 lossiness of `ValueExt::keys`: a lone surrogate
// key becomes U+FFFD in Rust and no longer indexes the original property.
#[wasm_bindgen_test]
fn keys_replace_lone_surrogates() {
	let object = js::obj! {};
	let key = js::call!(globalThis->String.fromCharCode(0xD800)).unwrap();
	js::set!(object->[key] = "hit").unwrap();
	let lossy = object.keys().unwrap().remove(0);
	assert_eq!(lossy, "\u{FFFD}");
	assert!(js::get!(object->[lossy]).unwrap().is_undefined());
	let raw = js::call!(globalThis->Object.keys(object)).unwrap();
	assert_eq!(js::get!(object->[js::get!(raw->[0]).unwrap()]).unwrap(), "hit");
}

#[wasm_bindgen_test]
fn number_matches_js_number_conversion() {
	let number = |value: Value| value.number().unwrap();
	assert_eq!(number(" 12 ".into()), 12.0);
	assert_eq!(number("".into()), 0.0);
	assert_eq!(number("0x10".into()), 16.0);
	assert_eq!(number(true.into()), 1.0);
	assert_eq!(number(Value::NULL), 0.0);
	assert!(number(Value::UNDEFINED).is_nan());
	assert!(number("12px".into()).is_nan());
	assert_eq!(number(js::obj! { valueOf: js::func!(|| 5) }), 5.0);
	let symbol = js::call!(globalThis->Symbol("s")).unwrap();
	assert!(symbol.number().is_err());
}

// Verifies `ToInt32` wrapping and non-finite handling in `ValueExt::int`.
#[wasm_bindgen_test]
fn int_converts_like_webidl_long() {
	let int = |value: Value| value.int().unwrap();
	assert_eq!(int(1.9.into()), 1);
	assert_eq!(int((-1.9).into()), -1);
	assert_eq!(int(f64::NAN.into()), 0);
	assert_eq!(int(f64::INFINITY.into()), 0);
	assert_eq!(int(2_147_483_648.0.into()), i32::MIN);
	assert_eq!(int(4_294_967_301.0.into()), 5);
	assert_eq!(int("7".into()), 7);
}

#[wasm_bindgen_test]
fn message_prefers_string_then_message_then_conversion() {
	assert_eq!(Value::from("plain").message(), "plain");
	assert_eq!(js::error!("boom").message(), "boom");
	assert_eq!(js::obj! { message: 5 }.message(), "[object Object]");
	assert_eq!(Value::from(42).message(), "42");
	assert_eq!(Value::NULL.message(), "null");
	let hostile = js::obj! { toString: thrower("no") };
	assert!(!hostile.message().is_empty());
}

#[wasm_bindgen_test]
fn name_reads_error_name_or_empty() {
	let abort = js::new!(globalThis->DOMException("x", "AbortError")).unwrap();
	assert_eq!(abort.name(), "AbortError");
	let type_error = js::new!(globalThis->TypeError("x")).unwrap();
	assert_eq!(type_error.name(), "TypeError");
	assert_eq!(Value::NULL.name(), "");
	assert_eq!(js::obj! { name: 1 }.name(), "");
}

#[wasm_bindgen_test]
fn args_reads_positions_and_missing_as_undefined() {
	let args = Args::new(["a".into(), 1.into()]);
	assert_eq!(args.len(), 2);
	assert!(!args.is_empty());
	assert_eq!(args.get(0), "a");
	assert_eq!(args.get(1), 1);
	assert!(args.get(2).is_undefined());
	assert!(args.get(99).is_undefined());

	args.set(1, "b".into());
	assert_eq!(args.to_vec(), vec![Value::from("a"), Value::from("b")]);
	assert!(Args::new([]).is_empty());
}

#[wasm_bindgen_test]
fn args_shift_mutates_and_skip_copies() {
	let args = Args::new([1.into(), 2.into(), 3.into()]);
	let rest = args.skip(1);
	assert_eq!(rest.to_vec(), vec![Value::from(2), Value::from(3)]);
	assert_eq!(args.len(), 3);
	assert!(args.skip(5).is_empty());

	assert_eq!(args.shift(), 1);
	assert_eq!(args.len(), 2);
	assert_eq!(args.get(0), 2);
	assert!(Args::new([]).shift().is_undefined());
}

#[wasm_bindgen_test]
fn args_clone_is_independent() {
	let args = Args::new(["a".into()]);
	let copy = args.clone();
	copy.set(0, "b".into());
	copy.set(1, "c".into());
	assert_eq!(args.get(0), "a");
	assert_eq!(args.len(), 1);
	assert_eq!(copy.len(), 2);
}

#[wasm_bindgen_test]
fn args_converts_to_js_array() {
	let value = Value::from(Args::new(["x".into()]));
	assert!(js::is!(value, Array));
	assert_eq!(js::get!(value->length).unwrap(), 1);
	assert_eq!(js::get!(value->[0]).unwrap(), "x");
}

#[wasm_bindgen_test]
fn string_matches_js_string_conversion() {
	assert_eq!(js::string(&12.into()).unwrap(), "12");
	assert_eq!(js::string(&Value::NULL).unwrap(), "null");
	assert_eq!(js::string(&Value::UNDEFINED).unwrap(), "undefined");
	assert_eq!(js::string(&js::arr![1, "a"]).unwrap(), "1,a");

	let symbol = js::call!(globalThis->Symbol("tag")).unwrap();
	assert_eq!(js::string(&symbol).unwrap(), "Symbol(tag)");

	let throwing = js::obj! { toString: thrower("no string") };
	assert_eq!(js::string(&throwing).unwrap_err().message(), "no string");
	let bare = js::call!(globalThis->Object.create(Value::NULL)).unwrap();
	assert!(js::string(&bare).is_err());
}

#[wasm_bindgen_test]
fn dict_has_no_prototype() {
	let dict = js::dict();
	assert!(js::call!(globalThis->Object.getPrototypeOf(dict)).unwrap().is_null());
	assert!(!js::has!(dict->toString).unwrap());
	js::set!(dict->key = 1).unwrap();
	assert_eq!(js::get!(dict->key).unwrap(), 1);
}

#[wasm_bindgen_test]
fn object_builds_from_pairs_without_proto_setter() {
	let object = js::object([("a", 1), ("__proto__", 2)]);
	assert_eq!(js::get!(object->a).unwrap(), 1);
	assert!(js::own!(object->["__proto__"]).unwrap());
	let prototype = js::call!(globalThis->Object.getPrototypeOf(object)).unwrap();
	assert_eq!(prototype, js::get!(globalThis->Object.prototype).unwrap());
	assert!(js::object(Vec::<(String, Value)>::new()).entries().unwrap().is_empty());
}

// Constructors have an own `prototype` property and must not be bound by `get_bound`.
#[wasm_bindgen_test]
fn get_bound_returns_constructors_unbound() {
	let global = js::get!(globalThis->globalThis).unwrap();
	for (name, member) in
		[("Object", "keys"), ("Promise", "resolve"), ("Array", "isArray")]
	{
		let original = js::get!(global->[name]).unwrap();
		let value = js::get_bound(&global, &name.into()).unwrap();
		assert_eq!(value, original, "{name}");
		assert!(js::get!(value->[member]).unwrap().is_function(), "{name}.{member}");
	}
}

#[wasm_bindgen_test]
fn get_bound_binds_methods_to_target() {
	let map = js::new!(globalThis->Map()).unwrap();
	js::call!(map->set("k", "v")).unwrap();
	let get = js::get_bound(&map, &"get".into()).unwrap();
	assert_eq!(js::call!(get->("k")).unwrap(), "v");
}

#[wasm_bindgen_test]
fn get_bound_returns_non_functions_as_is() {
	let inner = js::obj! {};
	let object = js::obj! { n: 1, o: inner };
	assert_eq!(js::get_bound(&object, &"n".into()).unwrap(), 1);
	assert_eq!(js::get_bound(&object, &"o".into()).unwrap(), inner);
	assert!(js::get_bound(&object, &"missing".into()).unwrap().is_undefined());
}

#[wasm_bindgen_test]
fn get_bound_binds_this_aware_closures() {
	let object = js::obj! { x: 1, read: js::func!(|this| js::get!(this->x)) };
	let read = js::get_bound(&object, &"read".into()).unwrap();
	assert_eq!(js::call!(read->()).unwrap(), 1);
}
