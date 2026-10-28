//! Browser integration tests for path macros (`get!`, `call!`, `set!`, `def!`, `del!`,
//! `update!`, `has!`, `own!`, `new!`, `wait!`, `is!`, `type_of!`, `keys!`, `entries!`)
//! and builder macros (`obj!`, `arr!`, `val!`, `func!`, `proxy!`, `error!`).

#![cfg(target_arch = "wasm32")]

mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::{counter, sleep, thrower};
use js::{Args, Value, ValueExt as _};
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
fn get_follows_dot_and_bracket_paths() {
	let object = js::obj! { a: js::obj! { b: 1 } };
	assert_eq!(js::get!(object->a.b).unwrap(), 1);
	assert_eq!(js::get!(object->["a"].b).unwrap(), 1);
	assert_eq!(js::get!(object->a["b"]).unwrap(), 1);
	let key: Value = "a".into();
	assert_eq!(js::get!(object->[key].b).unwrap(), 1);
	assert!(js::get!(object->missing).unwrap().is_undefined());
}

// Exercises `__js_scan!` munching arbitrary Rust root expressions before `->`.
#[wasm_bindgen_test]
fn get_accepts_global_and_expression_roots() {
	assert_eq!(js::get!(globalThis->Math.PI).unwrap(), std::f64::consts::PI);
	struct Holder {
		value: Value,
	}
	let holder = Holder { value: js::obj! { x: "field" } };
	assert_eq!(js::get!(holder.value->x).unwrap(), "field");
	let pair = (js::obj! { y: "tuple" }, 0);
	assert_eq!(js::get!(pair.0->y).unwrap(), "tuple");
	let args = Args::new([js::obj! { z: "args" }]);
	assert_eq!(js::get!(args.get(0)->z).unwrap(), "args");
}

#[wasm_bindgen_test]
fn get_optional_chain_short_circuits_on_nullish() {
	let object = js::obj! { a: js::obj! { b: 1 }, n: Value::NULL };
	assert!(js::get!(object->missing.x).is_err());
	assert!(js::get!(object->missing?.x).unwrap().is_undefined());
	assert!(js::get!(object->n?.x).unwrap().is_undefined());
	assert_eq!(js::get!(object->a?.b).unwrap(), 1);
	assert!(js::get!(object->a?.c?.d).unwrap().is_undefined());
}

// Raw identifiers (`r#type`) strip `r#` in `__js_key!`, while string literals keep it.
#[wasm_bindgen_test]
fn raw_identifier_keys_drop_their_prefix() {
	let object = js::obj! { r#type: "raw", "r#type": "literal" };
	assert_eq!(js::get!(object->type).unwrap(), "raw");
	assert_eq!(js::get!(object->r#type).unwrap(), "raw");
	assert_eq!(js::get!(object->["r#type"]).unwrap(), "literal");
	assert_eq!(js::get!(object->r#type?.length).unwrap(), 3);
	js::set!(object->r#type = "set").unwrap();
	assert_eq!(js::get!(object->["type"]).unwrap(), "set");

	let value = js::val!({ r#match: 1, nested: { r#type: 2 } });
	assert_eq!(value.keys().unwrap(), ["match", "nested"]);
	assert_eq!(js::get!(value->nested.type).unwrap(), 2);
}

// Once `?.` encounters `null` or `undefined`, the rest of the path is skipped across
// subsequent `.`, `()`, `has!`, `own!`, and `del!` steps.
#[wasm_bindgen_test]
fn optional_chain_skips_rest_of_path() {
	let root = js::obj! { a: Value::NULL, some: js::obj! { b: js::obj! { c: 1 } } };
	assert!(js::get!(root->a?.b.c).unwrap().is_undefined());
	assert!(js::call!(root->a?.b()).unwrap().is_undefined());
	assert!(js::call!(root->a?.b.c()).unwrap().is_undefined());
	assert!(js::get!(root->missing?.().length).unwrap().is_undefined());

	assert_eq!(js::get!(root->some?.b.c).unwrap(), 1);
	assert!(js::get!(root->some?.missing.c).is_err());

	assert!(!js::has!(root->a?.b.c).unwrap());
	assert!(!js::own!(root->a?.b).unwrap());
	assert!(js::has!(root->some?.b.c).unwrap());
	js::del!(root->a?.b.c).unwrap();
}

#[wasm_bindgen_test]
fn call_invokes_methods_with_receiver() {
	let map = js::new!(globalThis->Map()).unwrap();
	js::call!(map->set("k", "v")).unwrap();
	assert_eq!(js::call!(map->get("k")).unwrap(), "v");
	assert_eq!(js::get!(map->size).unwrap(), 1);
	assert_eq!(js::call!(map->["get"]("k")).unwrap(), "v");
	js::call!(map->set("o", js::obj! { n: 1 })).unwrap();
	assert_eq!(js::get!(map->get("o").n).unwrap(), 1);
	assert!(js::call!(map->entries().next()).unwrap().is_object());
}

// Native JS indexing in `r#macro::get` auto-boxes primitives when reading properties
// or calling methods on strings/numbers.
#[wasm_bindgen_test]
fn get_reads_properties_of_primitives() {
	let text = Value::from("v");
	assert_eq!(js::get!(text->length).unwrap(), 1);
	let map = js::new!(globalThis->Map()).unwrap();
	js::call!(map->set("k", "v")).unwrap();
	assert_eq!(js::call!(map->get("k").toUpperCase()).unwrap(), "V");
}

// Verifies `..rest` spread for `Args`, `Vec<Value>`, `[Value; N]`, and `&[Value]`.
#[wasm_bindgen_test]
fn call_spreads_rest_arguments() {
	let args = Args::new([1.into(), 5.into(), 3.into()]);
	assert_eq!(js::call!(globalThis->Math.max(..args)).unwrap(), 5);
	let vec: Vec<Value> = vec![1.into(), 7.into()];
	assert_eq!(js::call!(globalThis->Math.max(..vec)).unwrap(), 7);
	let array: [Value; 2] = [9.into(), 2.into()];
	assert_eq!(js::call!(globalThis->Math.max(..array)).unwrap(), 9);
	let slice: &[Value] = &array;
	assert_eq!(js::call!(globalThis->Math.max(..slice)).unwrap(), 9);
	let empty: Vec<Value> = vec![];
	assert_eq!(js::call!(globalThis->Math.max(..empty)).unwrap(), f64::NEG_INFINITY);
}

// As in JS, `a.f(x)` reads `a.f` before it evaluates `x`.
#[wasm_bindgen_test]
fn method_call_reads_member_before_arguments() {
	let (count, tick) = counter();
	let arg = || {
		tick();
		1
	};
	assert!(js::call!(Value::NULL->f(arg())).is_err());
	assert!(js::call!(Value::UNDEFINED->a.f(arg())).is_err());
	assert_eq!(count.get(), 0);
	assert_eq!(js::call!(globalThis->Math.abs(arg())).unwrap(), 1);
	assert_eq!(count.get(), 1);
}

#[wasm_bindgen_test]
fn call_invokes_function_values_without_this() {
	let echo = js::func!(|a| a);
	assert_eq!(js::call!(echo->("x")).unwrap(), "x");
	let this = js::func!(|this| this);
	assert!(js::call!(this->()).unwrap().is_undefined());
	assert!(js::call!(globalThis->Math.PI()).is_err());
}

#[wasm_bindgen_test]
fn call_rejects_non_functions_with_key_name() {
	let object = js::obj! { n: 1 };
	let error = js::call!(object->nope()).unwrap_err();
	assert!(js::is!(error, TypeError));
	assert_eq!(error.message(), "nope is not a function");
	assert_eq!(js::call!(object->n()).unwrap_err().message(), "n is not a function");
}

// `f?.()` skips `null` and `undefined`, but still errors if the property holds a
// non-callable value like `1`.
#[wasm_bindgen_test]
fn optional_call_skips_missing_functions() {
	let object = js::obj! { f: js::func!(|a| a), n: 1, z: Value::NULL };
	assert_eq!(js::call!(object->f?.("v")).unwrap(), "v");
	assert!(js::call!(object->missing?.()).unwrap().is_undefined());
	assert!(js::call!(object->z?.()).unwrap().is_undefined());
	assert_eq!(js::call!(object->["f"]?.("w")).unwrap(), "w");
	let error = js::call!(object->n?.()).unwrap_err();
	assert!(js::is!(error, TypeError));
	assert_eq!(error.message(), "n is not a function");
}

#[wasm_bindgen_test]
fn has_checks_key_in_object() {
	let object = js::obj! { a: js::obj! { b: 1 } };
	assert!(js::has!(object->a).unwrap());
	assert!(js::has!(object->a.b).unwrap());
	assert!(js::has!(object->["a"]).unwrap());
	assert!(!js::has!(object->zzz).unwrap());
	assert!(!js::has!(object->a.zzz).unwrap());
	let map = js::new!(globalThis->Map()).unwrap();
	assert!(js::has!(map->get).unwrap());
	assert!(js::has!(globalThis->Math).unwrap());
}

#[wasm_bindgen_test]
fn own_checks_only_own_keys() {
	let parent = js::obj! { inherited: 1 };
	let child = js::call!(globalThis->Object.create(parent)).unwrap();
	js::set!(child->mine = 2).unwrap();
	assert!(js::own!(child->mine).unwrap());
	assert!(!js::own!(child->inherited).unwrap());
	assert!(js::has!(child->inherited).unwrap());
	let key: Value = "mine".into();
	assert!(js::own!(child->[key]).unwrap());
	assert!(js::own!(Value::from("ab")->length).unwrap());
	assert!(js::own!(Value::NULL->x).is_err());
}

#[wasm_bindgen_test]
fn set_assigns_and_reports_rejection() {
	let object = js::obj! { a: js::obj! {} };
	js::set!(object->a.c = 2).unwrap();
	assert_eq!(js::get!(object->a.c).unwrap(), 2);
	js::set!(object->["d"] = "x").unwrap();
	assert_eq!(js::get!(object->d).unwrap(), "x");
	let key: Value = "e".into();
	js::set!(object->a[key] = true).unwrap();
	assert_eq!(js::get!(object->a.e).unwrap(), true);

	let frozen = js::call!(globalThis->Object.freeze(js::obj! { x: 1 })).unwrap();
	let error = js::set!(frozen->x = 2).unwrap_err();
	assert!(js::is!(error, TypeError));
	assert_eq!(error.message(), "cannot assign to property x");
	assert_eq!(js::get!(frozen->x).unwrap(), 1);
}

#[wasm_bindgen_test]
fn def_installs_properties_and_reports_rejection() {
	let reads = Rc::new(Cell::new(0));
	let counter = reads.clone();
	let object = js::obj! {};
	let getter = js::func!(|| {
		counter.set(counter.get() + 1);
		counter.get()
	});
	js::def!(object->answer, { get: getter, enumerable: true }).unwrap();
	assert_eq!(js::get!(object->answer).unwrap(), 1);
	assert_eq!(js::get!(object->answer).unwrap(), 2);
	assert_eq!(reads.get(), 2);

	js::def!(object->["hidden"], { value: 7 }).unwrap();
	assert_eq!(js::get!(object->hidden).unwrap(), 7);
	let keys = js::call!(globalThis->Object.keys(object)).unwrap();
	assert_eq!(js::get!(keys->length).unwrap(), 1);
	assert!(js::set!(object->hidden = 8).is_err());

	let frozen = js::call!(globalThis->Object.freeze(js::obj! {})).unwrap();
	let error = js::def!(frozen->x, { value: 1 }).unwrap_err();
	assert_eq!(error.message(), "cannot define property x");
	assert!(js::def!(Value::from(1)->x, { value: 1 }).is_err());
}

#[wasm_bindgen_test]
fn del_removes_properties_and_reports_rejection() {
	let object = js::obj! { a: 1, nested: js::obj! { b: 2 } };
	js::del!(object->a).unwrap();
	assert!(!js::own!(object->a).unwrap());
	js::del!(object->nested["b"]).unwrap();
	assert!(!js::own!(object->nested.b).unwrap());
	js::del!(object->missing).unwrap();

	let fixed = js::obj! {};
	js::def!(fixed->x, { value: 1 }).unwrap();
	let error = js::del!(fixed->x).unwrap_err();
	assert_eq!(error.message(), "cannot delete property x");
	assert!(js::is!(error, TypeError));
	assert!(js::del!(Value::from(1)->x).is_err());
}

// `update!` must evaluate intermediate path steps (`store->find()`) only once.
#[wasm_bindgen_test]
fn update_reads_once_and_writes_back() {
	let stats = js::obj! { count: 1 };
	let new = js::update!(stats->count, |old| old.as_f64().unwrap() + 1.0).unwrap();
	assert_eq!(new, 2);
	assert_eq!(js::get!(stats->count).unwrap(), 2);
	js::update!(stats->["label"], |old| format!("{}!", js::string(&old).unwrap()))
		.unwrap();
	assert_eq!(js::get!(stats->label).unwrap(), "undefined!");

	let (calls, count) = counter();
	let store = js::obj! {
		find: js::func!(move || -> Value {
			count();
			js::obj! { hits: 10 }
		}),
	};
	let hits =
		js::update!(store->find().hits, |old| old.as_f64().unwrap() * 2.0).unwrap();
	assert_eq!(hits, 20);
	assert_eq!(calls.get(), 1);

	let frozen = js::call!(globalThis->Object.freeze(js::obj! { x: 1 })).unwrap();
	assert!(js::update!(frozen->x, |_| 2).is_err());
	assert_eq!(js::get!(frozen->x).unwrap(), 1);
}

#[wasm_bindgen_test]
fn new_constructs_instances() {
	let map = js::new!(globalThis->Map()).unwrap();
	assert!(js::is!(map, Map));
	let date = js::new!(globalThis->Date(1000)).unwrap();
	assert_eq!(js::call!(date->getTime()).unwrap(), 1000);
	let constructor = js::get!(globalThis->Set).unwrap();
	assert!(js::is!(js::new!(constructor->()).unwrap(), Set));

	let error = js::new!(globalThis->Math.PI()).unwrap_err();
	assert_eq!(error.message(), "value is not a constructor");
	assert!(js::new!(globalThis->Math.abs()).is_err());
}

#[wasm_bindgen_test]
fn obj_builds_objects_from_keys() {
	let object = js::obj! { a: 1, "b c": "x", nested: js::obj! { d: true }, };
	assert_eq!(js::get!(object->a).unwrap(), 1);
	assert_eq!(js::get!(object->["b c"]).unwrap(), "x");
	assert_eq!(js::get!(object->nested.d).unwrap(), true);
	assert_eq!(js::get!(globalThis->Object.keys(object).length).unwrap(), 3);

	let empty = js::obj! {};
	assert!(js::is!(empty, Object));
	assert_eq!(js::get!(globalThis->Object.keys(empty).length).unwrap(), 0);
}

#[wasm_bindgen_test]
fn obj_spread_copies_shallowly_and_overrides() {
	let inner = js::obj! {};
	let source = js::obj! { a: 1, b: 2, inner: inner };
	let copy = js::obj! { ..source, b: 3, c: 4 }.unwrap();
	assert_ne!(copy, source);
	assert_eq!(js::get!(copy->a).unwrap(), 1);
	assert_eq!(js::get!(copy->b).unwrap(), 3);
	assert_eq!(js::get!(copy->c).unwrap(), 4);
	assert_eq!(js::get!(copy->inner).unwrap(), inner);
	assert_eq!(js::get!(source->b).unwrap(), 2);
	assert!(js::get!(source->c).unwrap().is_undefined());

	let plain = js::obj! { ..source }.unwrap();
	assert_ne!(plain, source);
	assert_eq!(js::get!(plain->a).unwrap(), 1);
}

#[wasm_bindgen_test]
fn obj_takes_computed_keys() {
	let name = "data-id".to_owned();
	let symbol = js::call!(globalThis->Symbol("tag")).unwrap();
	let object = js::obj! { [name]: 1, [2]: "two", [symbol]: true, plain: 0 }.unwrap();
	assert_eq!(js::get!(object->[name]).unwrap(), 1);
	assert_eq!(js::get!(object->["2"]).unwrap(), "two");
	assert_eq!(js::get!(object->[symbol]).unwrap(), true);
	assert_eq!(js::get!(object->plain).unwrap(), 0);

	let copy = js::obj! { ..object, [name]: 9 }.unwrap();
	assert_eq!(js::get!(copy->[name]).unwrap(), 9);
	assert_eq!(js::get!(copy->plain).unwrap(), 0);
}

// Ensures `"__proto__"` via literal key, computed key, or `..spread` is defined as an
// own property and never mutates the object's prototype.
#[wasm_bindgen_test]
fn obj_never_sets_prototype() {
	let planted = js::obj! { polluted: true };
	let key = "__proto__";
	let json = r#"{"__proto__": {"polluted": true}}"#;
	let parsed = js::call!(globalThis->JSON.parse(json)).unwrap();
	let objects = [
		js::obj! { "__proto__": planted },
		js::obj! { [key]: planted }.unwrap(),
		js::obj! { ..parsed }.unwrap(),
	];
	let base = js::get!(globalThis->Object.prototype).unwrap();
	for object in objects {
		assert!(js::own!(object->["__proto__"]).unwrap());
		let prototype = js::call!(globalThis->Object.getPrototypeOf(object.clone()));
		assert_eq!(prototype.unwrap(), base);
		assert!(js::get!(object->polluted).unwrap().is_undefined());
	}
}

#[wasm_bindgen_test]
fn val_builds_nested_literals() {
	let symbol = js::call!(globalThis->Symbol()).unwrap();
	let label = "close";
	let value = js::val!({
		id: 1,
		"aria-label": label,
		[symbol]: true,
		nested: { ok: null, skip: undefined, list: [1, [2, 3], { deep: "x" }] },
		empty: { inner: [] },
		call: js::func!(|| 7),
	})
	.unwrap();
	assert_eq!(js::get!(value->id).unwrap(), 1);
	assert_eq!(js::get!(value->["aria-label"]).unwrap(), "close");
	assert_eq!(js::get!(value->[symbol]).unwrap(), true);
	assert!(js::get!(value->nested.ok).unwrap().is_null());
	assert!(js::own!(value->nested.skip).unwrap());
	assert!(js::get!(value->nested.skip).unwrap().is_undefined());
	assert_eq!(js::get!(value->nested.list[1][0]).unwrap(), 2);
	assert_eq!(js::get!(value->nested.list[2].deep).unwrap(), "x");
	assert_eq!(js::get!(value->empty.inner.length).unwrap(), 0);
	assert_eq!(js::call!(value->call()).unwrap(), 7);

	assert_eq!(js::val!(5), 5);
	assert!(js::val!(null).is_null());
	assert!(js::val!(undefined).is_undefined());
	assert_eq!(js::get!(js::val!([])->length).unwrap(), 0);
	assert!(js::val!({}).keys().unwrap().is_empty());
}

#[wasm_bindgen_test]
fn val_spreads_anywhere_in_order() {
	let first = js::obj! { a: 1, b: 1 };
	let second = js::obj! { b: 2, c: 2 };
	let value =
		js::val!({ a: 0, ..first, b: 9, ..second, ..Value::NULL, d: 4 }).unwrap();
	assert_eq!(value.keys().unwrap(), ["a", "b", "c", "d"]);
	assert_eq!(js::get!(value->a).unwrap(), 1);
	assert_eq!(js::get!(value->b).unwrap(), 2);
	assert_eq!(js::get!(value->c).unwrap(), 2);
	assert_eq!(js::get!(value->d).unwrap(), 4);

	let middle = vec![2, 3];
	let list = js::val!([1, ..middle, 4, ..Vec::<i32>::new()]);
	assert_eq!(list.to_list().unwrap().to_vec(), [1, 2, 3, 4].map(Value::from));
}

// Verifies that compound Rust expressions starting with `[...]` or `{...}` (such as
// `[pair[0], pair[1]][0]` or `{ 1 } + 1`) are not mistaken for nested JS literals in
// `__js_val_fields!` and `__js_val_items!`.
#[wasm_bindgen_test]
fn val_reads_rust_expressions_and_keeps_proto_own() {
	let pair = [10, 20];
	let value = js::val!({
		len: pair.len(),
		first: [pair[0], pair[1]][0],
		sum: { 1 } + 1,
		block: ({ let x = 2; x * 3 }),
		"__proto__": { polluted: true },
	});
	assert_eq!(js::get!(value->len).unwrap(), 2);
	assert_eq!(js::get!(value->first).unwrap(), 10);
	assert_eq!(js::get!(value->sum).unwrap(), 2);
	assert_eq!(js::get!(value->block).unwrap(), 6);
	assert!(js::own!(value->["__proto__"]).unwrap());
	assert!(js::get!(value->polluted).unwrap().is_undefined());

	let items = js::val!([pair.len(), [pair[1]][0], null]);
	assert_eq!(
		items.to_list().unwrap().to_vec(),
		[Value::from(2), Value::from(20), Value::NULL]
	);
}

#[wasm_bindgen_test]
fn builders_return_result_only_for_spreads_and_computed_keys() {
	let source = js::obj! { a: 1 };
	let key = "k";
	let _: Value = js::obj! { a: 1, "b": 2, c: js::obj! { d: 3 } };
	let _: js::Result = js::obj! { ..source };
	let _: js::Result = js::obj! { a: 1, [key]: 2 };
	let _: Value = js::val!({ a: [1, { b: null }], c: [..[1, 2]] });
	let _: Value = js::val!([..[1, 2], { a: [] }]);
	let _: js::Result = js::val!({ a: [1, { ..source }] });
	let _: js::Result = js::val!([[{ b: { [key]: 1 } }]]);
}

#[wasm_bindgen_test]
fn obj_spreads_anywhere_in_order() {
	let first = js::obj! { a: 1, b: 1 };
	let second = js::obj! { b: 2, c: 2 };
	let value = js::obj! { a: 0, ..first, b: 9, ..second, ..Value::NULL, d: 4 }.unwrap();
	assert_eq!(value.keys().unwrap(), ["a", "b", "c", "d"]);
	assert_eq!(js::get!(value->a).unwrap(), 1);
	assert_eq!(js::get!(value->b).unwrap(), 2);
	assert_eq!(js::get!(value->c).unwrap(), 2);
	assert_eq!(js::get!(value->d).unwrap(), 4);
}

#[wasm_bindgen_test]
fn builders_return_errors_thrown_by_page_code() {
	let getter = js::obj! {};
	js::def!(getter->x, { get: thrower("getter"), enumerable: true }).unwrap();
	let traps = js::obj! { ownKeys: thrower("ownKeys") };
	let proxy = js::new!(globalThis->Proxy(js::obj! {}, traps)).unwrap();
	let key = js::obj! { toString: thrower("toString") };

	let cell = std::cell::RefCell::new(0);
	{
		let mut guard = cell.borrow_mut();
		assert_eq!(js::obj! { ..getter }.unwrap_err().message(), "getter");
		assert_eq!(js::obj! { a: 1, ..proxy }.unwrap_err().message(), "ownKeys");
		assert_eq!(js::obj! { [key]: 1 }.unwrap_err().message(), "toString");
		assert_eq!(js::val!({ a: [{ ..getter }] }).unwrap_err().message(), "getter");
		assert_eq!(js::val!([{ ..proxy }]).unwrap_err().message(), "ownKeys");
		assert_eq!(js::val!({ b: { [key]: 1 } }).unwrap_err().message(), "toString");
		*guard += 1;
	}
	assert_eq!(*cell.borrow(), 1);
}

// JS converts a computed key before it evaluates the value.
#[wasm_bindgen_test]
fn computed_key_errors_skip_the_value() {
	let key = js::obj! { toString: thrower("toString") };
	let (count, tick) = counter();
	let value = || {
		tick();
		1
	};
	assert!(js::obj! { [key]: value() }.is_err());
	assert!(js::val!({ [key]: value() }).is_err());
	assert_eq!(count.get(), 0);

	let symbol = js::call!(globalThis->Symbol("s")).unwrap();
	let to_primitive = js::get!(globalThis->Symbol.toPrimitive).unwrap();
	let wrapped =
		js::obj! { [to_primitive]: js::func!(move || symbol.clone()) }.unwrap();
	let object = js::obj! { [wrapped]: value(), [1.5]: 2, [true]: 3 }.unwrap();
	assert_eq!(count.get(), 1);
	assert_eq!(js::get!(object->["true"]).unwrap(), 3);
	assert_eq!(js::get!(object->["1.5"]).unwrap(), 2);
	assert_eq!(
		js::get!(globalThis->Object.getOwnPropertySymbols(object).length).unwrap(),
		1
	);
}

// The fallible `val!` path must parse Rust expressions the same way as the plain one.
#[wasm_bindgen_test]
fn val_with_computed_keys_reads_rust_expressions() {
	let pair = [10, 20];
	let key = "k";
	let value = js::val!({
		[key]: [pair[0], pair[1]][0],
		sum: { 1 } + 1,
		list: [[pair[1]][0], null, undefined, { x: 1 }],
		"__proto__": { polluted: true },
	})
	.unwrap();
	assert_eq!(js::get!(value->k).unwrap(), 10);
	assert_eq!(js::get!(value->sum).unwrap(), 2);
	assert_eq!(js::get!(value->list[0]).unwrap(), 20);
	assert!(js::get!(value->list[1]).unwrap().is_null());
	assert!(js::get!(value->list[2]).unwrap().is_undefined());
	assert_eq!(js::get!(value->list[3].x).unwrap(), 1);
	assert!(js::own!(value->["__proto__"]).unwrap());
	assert!(js::get!(value->polluted).unwrap().is_undefined());
}

#[wasm_bindgen_test]
fn func_binds_fixed_parameters() {
	let pair = js::func!(|a, b| js::arr![a, b]);
	let result = js::call!(pair->("x")).unwrap();
	assert_eq!(js::get!(result->[0]).unwrap(), "x");
	assert!(js::get!(result->[1]).unwrap().is_undefined());
	let result = js::call!(pair->("x", "y", "z")).unwrap();
	assert_eq!(js::get!(result->length).unwrap(), 2);
	assert_eq!(js::get!(result->[1]).unwrap(), "y");

	let answer = js::func!(|| 42);
	assert_eq!(js::call!(answer->()).unwrap(), 42);
	assert_eq!(js::call!(answer->(1, 2)).unwrap(), 42);
}

#[wasm_bindgen_test]
fn func_collects_rest_arguments() {
	let tail = js::func!(|_first, ..rest| Value::from(rest));
	let rest = js::call!(tail->(1, 2, 3)).unwrap();
	assert_eq!(js::get!(rest->length).unwrap(), 2);
	assert_eq!(js::get!(rest->[0]).unwrap(), 2);

	let count = js::func!(|..all| all.len());
	assert_eq!(js::call!(count->()).unwrap(), 0);
	assert_eq!(js::call!(count->("a", "b")).unwrap(), 2);
}

#[wasm_bindgen_test]
fn func_receives_this_when_named() {
	let method = js::func!(|this, a| js::arr![this, a]);
	let object = js::obj! { method: method };
	let result = js::call!(object->method("arg")).unwrap();
	assert_eq!(js::get!(result->[0]).unwrap(), object);
	assert_eq!(js::get!(result->[1]).unwrap(), "arg");

	let result = js::call!(method->call("ctx", "arg")).unwrap();
	assert_eq!(js::get!(result->[0]).unwrap(), "ctx");
	assert_eq!(js::get!(result->[1]).unwrap(), "arg");
}

#[wasm_bindgen_test]
fn func_throws_err_as_js_exception() {
	let checked = js::func!(|a| -> js::Result {
		if a.is_undefined() { Err(js::error!("missing a")) } else { Ok(a) }
	});
	assert_eq!(js::call!(checked->("v")).unwrap(), "v");
	assert_eq!(js::call!(checked->()).unwrap_err().message(), "missing a");
	assert_eq!(js::call!(thrower("t")->()).unwrap_err().message(), "t");
}

#[wasm_bindgen_test]
fn func_captures_environment_by_move() {
	let (count, tick) = counter();
	let bump = js::func!(move || tick());
	js::call!(bump->()).unwrap();
	js::call!(bump->()).unwrap();
	assert_eq!(count.get(), 2);
}

#[wasm_bindgen_test]
async fn func_async_returns_promise() {
	let later = js::func!(async |a| async move {
		sleep(0).await;
		Ok(a)
	});
	let promise = js::call!(later->("v")).unwrap();
	assert!(js::is!(promise, Promise));
	assert_eq!(js::resolve(promise).await.unwrap(), "v");

	let failing = js::func!(async || async { Err::<Value, Value>("no".into()) });
	assert_eq!(js::wait!(failing->()).await.unwrap_err(), "no");
}

#[wasm_bindgen_test]
fn func_mut_keeps_state_between_calls() {
	let mut total = 0.0;
	let add = js::func!(mut move |x| {
		total += x.as_f64().unwrap();
		total
	});
	assert_eq!(js::call!(add->(1)).unwrap(), 1);
	assert_eq!(js::call!(add->(2)).unwrap(), 3);
	assert_eq!(js::call!(add->(4)).unwrap(), 7);
}

#[wasm_bindgen_test]
fn func_once_moves_captures_and_rejects_second_call() {
	let owned = String::from("moved");
	let take = js::func!(once move || owned);
	assert_eq!(js::call!(take->()).unwrap(), "moved");
	assert!(js::call!(take->()).is_err());

	let echo = js::func!(once |x| -> js::Value { x });
	assert_eq!(js::call!(echo->("arg")).unwrap(), "arg");
}

#[wasm_bindgen_test]
async fn func_combines_this_with_mut_once_and_async() {
	let mut calls = 0;
	let count = js::func!(mut move |this, a| {
		calls += 1;
		js::arr![this, a, calls]
	});
	js::call!(count->call("ctx", "a")).unwrap();
	let result = js::call!(count->call("ctx", "b")).unwrap();
	assert_eq!(js::get!(result->[0]).unwrap(), "ctx");
	assert_eq!(js::get!(result->[1]).unwrap(), "b");
	assert_eq!(js::get!(result->[2]).unwrap(), 2);

	let owned = String::from("moved");
	let take = js::func!(once move |this| js::arr![this, owned]);
	let result = js::call!(take->call("ctx")).unwrap();
	assert_eq!(js::get!(result->[0]).unwrap(), "ctx");
	assert_eq!(js::get!(result->[1]).unwrap(), "moved");
	assert!(js::call!(take->call("ctx")).is_err());

	let later = js::func!(async |this, a| async move {
		sleep(0).await;
		Ok(js::arr![this, a])
	});
	let object = js::obj! { later: later };
	let result = js::wait!(object->later("v")).await.unwrap();
	assert_eq!(js::get!(result->[0]).unwrap(), object);
	assert_eq!(js::get!(result->[1]).unwrap(), "v");
}

#[wasm_bindgen_test]
fn is_checks_instance_of_global_class() {
	let map = js::new!(globalThis->Map()).unwrap();
	assert!(js::is!(map, Map));
	assert!(js::is!(map, Object));
	assert!(!js::is!(map, Array));
	assert!(!js::is!(Value::from("s"), String));
	assert!(!js::is!(Value::NULL, Object));
	assert!(!js::is!(map, NoSuchClass));
}

#[wasm_bindgen_test]
fn is_checks_instance_of_constructor_path() {
	let map = js::new!(globalThis->Map()).unwrap();
	assert!(js::is!(map, globalThis->Map));
	assert!(!js::is!(map, globalThis->Array));

	let has_instance = js::get!(globalThis->Symbol.hasInstance).unwrap();
	let anything = js::obj! {};
	js::set!(anything->[has_instance] = js::func!(|| true)).unwrap();
	let realm = js::obj! { Map: js::get!(globalThis->Map).unwrap(), Any: anything };
	assert!(js::is!(map, realm->Map));
	assert!(js::is!(Value::from(1), realm->["Any"]));
	assert!(!js::is!(map, realm->missing));
}

#[wasm_bindgen_test]
fn type_of_reports_every_typeof_result() {
	let values = js::obj! {
		undefined: Value::UNDEFINED,
		null: Value::NULL,
		boolean: true,
		number: 1.5,
		bigint: 1u64,
		string: "s",
		symbol: js::call!(globalThis->Symbol()).unwrap(),
		function: js::func!(|| ()),
		object: js::obj! {},
	};
	let expected = [
		("undefined", js::Type::Undefined),
		("null", js::Type::Object),
		("boolean", js::Type::Boolean),
		("number", js::Type::Number),
		("bigint", js::Type::BigInt),
		("string", js::Type::String),
		("symbol", js::Type::Symbol),
		("function", js::Type::Function),
		("object", js::Type::Object),
	];
	for (key, kind) in expected {
		assert_eq!(js::type_of!(values->[key]).unwrap(), kind, "{key}");
		let value = js::get!(values->[key]).unwrap();
		assert_eq!(kind.as_str(), value.js_typeof().as_string().unwrap(), "{key}");
	}
	assert!(js::type_of!(values->missing.deeper).is_err());
}

#[wasm_bindgen_test]
fn keys_and_entries_read_value_at_path() {
	let root = js::obj! { inner: js::obj! { x: 1, y: "two" }, empty: Value::NULL };
	assert_eq!(js::keys!(root->inner).unwrap(), ["x", "y"]);
	assert_eq!(
		js::entries!(root->inner).unwrap().to_vec(),
		vec![("x".to_owned(), Value::from(1)), ("y".to_owned(), Value::from("two"))]
	);
	assert!(js::keys!(root->empty).is_err());
	assert!(js::entries!(root->missing.deeper).is_err());
}

#[wasm_bindgen_test]
fn arr_builds_js_arrays() {
	let empty = js::arr![];
	assert!(js::is!(empty, Array));
	assert_eq!(js::get!(empty->length).unwrap(), 0);

	let mixed = js::arr![1, "a", true];
	assert_eq!(js::get!(mixed->length).unwrap(), 3);
	assert_eq!(js::get!(mixed->[0]).unwrap(), 1);
	assert_eq!(js::get!(mixed->[1]).unwrap(), "a");
	assert_eq!(js::get!(mixed->[2]).unwrap(), true);

	let bytes = [7u8, 8, 9];
	let spread = js::arr![..bytes.iter().copied()];
	assert_eq!(js::get!(spread->length).unwrap(), 3);
	assert_eq!(js::get!(spread->[2]).unwrap(), 9);
	let values: Vec<Value> = vec!["x".into()];
	assert_eq!(js::get!(js::arr![..values]->[0]).unwrap(), "x");
}

#[wasm_bindgen_test]
fn error_builds_js_error_with_message() {
	let name = "x";
	let error = js::error!("{name} must be a string");
	assert!(js::is!(error, Error));
	assert_eq!(js::get!(error->message).unwrap(), "x must be a string");
	assert_eq!(js::error!("{}-{}", 1, 2).message(), "1-2");
}

#[wasm_bindgen_test]
async fn wait_awaits_path_result() {
	assert_eq!(js::wait!(globalThis->Promise.resolve(3)).await.unwrap(), 3);
	assert_eq!(js::wait!(globalThis->Promise.reject("e")).await.unwrap_err(), "e");
	let object = js::obj! { a: js::obj! { b: 1 } };
	assert_eq!(js::wait!(object->a.b).await.unwrap(), 1);
	assert!(js::wait!(object->missing.x).await.is_err());
}

#[wasm_bindgen_test]
fn proxy_get_trap_intercepts_reads() {
	let target = js::obj! { a: 1 };
	let original = target.clone();
	let proxy = js::proxy!(target, {
		get: move |target: &Value, key: &Value| {
			assert_eq!(*target, original);
			if key.as_string().as_deref() == Some("a") {
				return Ok(Value::from("trapped"));
			}
			js::get!(target->[key])
		},
	})
	.unwrap();
	assert_eq!(js::get!(proxy->a).unwrap(), "trapped");
	assert!(js::get!(proxy->b).unwrap().is_undefined());

	let plain = js::proxy!(target, {}).unwrap();
	assert_eq!(js::get!(plain->a).unwrap(), 1);
}

#[wasm_bindgen_test]
fn proxy_set_and_delete_traps_can_refuse() {
	let target = js::obj! { a: 1, b: 2 };
	let proxy = js::proxy!(target, {
		set: |target: &Value, key: &Value, value: &Value| {
			if key.as_string().as_deref() == Some("a") {
				return Ok(false);
			}
			js::set!(target->[key] = value)?;
			Ok(true)
		},
		delete_property: |target: &Value, key: &Value| {
			if key.as_string().as_deref() == Some("a") {
				return Ok(false);
			}
			js::call!(globalThis->Reflect.deleteProperty(target, key)).map(|_| true)
		},
	})
	.unwrap();

	let error = js::set!(proxy->a = 9).unwrap_err();
	assert_eq!(error.message(), "cannot assign to property a");
	assert_eq!(js::get!(target->a).unwrap(), 1);
	js::set!(proxy->c = 3).unwrap();
	assert_eq!(js::get!(target->c).unwrap(), 3);

	let deleted = js::call!(globalThis->Reflect.deleteProperty(proxy, "a")).unwrap();
	assert_eq!(deleted, false);
	assert_eq!(js::get!(target->a).unwrap(), 1);
	let deleted = js::call!(globalThis->Reflect.deleteProperty(proxy, "b")).unwrap();
	assert_eq!(deleted, true);
	assert!(js::get!(target->b).unwrap().is_undefined());
}

#[wasm_bindgen_test]
fn proxy_has_trap_and_trap_errors() {
	let target = js::obj! { a: 1 };
	let proxy = js::proxy!(target, {
		has: |_target: &Value, key: &Value| {
			Ok(key.as_string().as_deref() == Some("ghost"))
		},
		get: |_target: &Value, _key: &Value| Err(js::error!("trap failed")),
	})
	.unwrap();
	assert!(js::has!(proxy->ghost).unwrap());
	assert!(!js::has!(proxy->a).unwrap());
	assert_eq!(js::get!(proxy->a).unwrap_err().message(), "trap failed");
}
