//! `Value` extension methods, `List` and variadic `Args`, `typeof` inspection, and JSON
//! conversions.

use std::marker::PhantomData;

use js_sys::{Array, JSON, Object, Reflect};
use serde::Serialize;
use serde::de::DeserializeOwned;
use wasm_bindgen::JsCast as _;

pub use wasm_bindgen::JsValue as Value;

pub use crate::r#macro::string;

/// Alias for `std::result::Result<T, Value>` where both the default success type and
/// the error type are a JavaScript `Value`.
pub type Result<T = Value> = std::result::Result<T, Value>;

/// Convenience conversions and property readers on a JavaScript `Value`.
pub trait ValueExt {
	/// Returns `None` if `self` is `null` or `undefined`, or `Some(self)` for any other
	/// value (including falsy values like `0`, `false`, and `""`).
	///
	/// # Examples
	///
	/// ```no_run
	/// use js::{Value, ValueExt as _};
	///
	/// assert!(Value::NULL.present().is_none());
	/// assert!(Value::UNDEFINED.present().is_none());
	/// assert_eq!(Value::from(0).present(), Some(Value::from(0)));
	/// ```
	fn present(self) -> Option<Value>;

	/// Extracts a Rust `String` if `self` is a JS string, or returns `""` without
	/// coercing non-string values.
	///
	/// Unpaired UTF-16 surrogates are replaced with U+FFFD. To coerce non-strings via
	/// `String(value)`, use `js::string`.
	///
	/// # Examples
	///
	/// ```no_run
	/// use js::{Value, ValueExt as _};
	///
	/// assert_eq!(Value::from("hello").text(), "hello");
	/// assert_eq!(Value::from(42).text(), "");
	/// ```
	fn text(&self) -> String;

	/// Extracts a Rust `String` if `self` is a JS string, or returns an `Error` with the
	/// message `"{what} must be a string"`.
	///
	/// # Errors
	///
	/// Returns `Err` if `self` is not a JS string.
	///
	/// # Examples
	///
	/// ```no_run
	/// use js::{Value, ValueExt as _};
	///
	/// assert_eq!(Value::from("ok").expect_text("label").unwrap(), "ok");
	/// assert!(Value::from(1).expect_text("label").is_err());
	/// ```
	fn expect_text(&self, what: &str) -> Result<String>;

	/// Copies an array-like JavaScript value into a `List` using
	/// `Array.prototype.slice.call(self)`.
	///
	/// `self.length` is coerced with ECMAScript `ToLength`: `NaN` and negative numbers
	/// become `0`, fractional numbers truncate toward zero, and numeric strings such as
	/// `"2"` are converted. Holes in sparse array-likes read as `undefined`.
	///
	/// Because `List` keeps its elements inside the JS `Array` and holds only a single
	/// `externref` table slot in Rust, copying a large page-controlled object does not
	/// risk aborting the wasm instance by filling the browser's `externref` table.
	///
	/// # Errors
	///
	/// Returns `Err` with a `TypeError` if `self` is `null` or `undefined`, with a
	/// `RangeError` from JS if `length` exceeds `2^32 - 1` (`4_294_967_295`) or is
	/// `+Infinity`, or with the thrown value if reading `length` or any index throws.
	///
	/// # Examples
	///
	/// ```no_run
	/// use js::{Value, ValueExt as _};
	///
	/// # fn example() -> js::Result<()> {
	/// let fake = js::obj! { length: 2, "0": "a", "1": "b" };
	/// assert_eq!(fake.to_list()?.to_vec(), vec![Value::from("a"), Value::from("b")]);
	/// assert!(Value::NULL.to_list().is_err());
	/// # Ok(())
	/// # }
	/// ```
	fn to_list(&self) -> Result<List>;

	/// Collects the elements of any JavaScript iterable (such as a `Set`, `Map`, or
	/// `Array`) into a `List` using JS array spread (`[...self]`).
	///
	/// Elements stay in the backing JS `Array` and are brought into Rust one at a time
	/// as the `List` is read or iterated. An endless iterator runs until the JS engine
	/// runs out of memory, as in JavaScript.
	///
	/// # Errors
	///
	/// Returns `Err` with the JS engine's `TypeError` if `self` is not iterable, or
	/// returns the thrown value if `[Symbol.iterator]` or `next()` throws.
	///
	/// # Examples
	///
	/// ```no_run
	/// use js::{Value, ValueExt as _};
	///
	/// # fn example() -> js::Result<()> {
	/// let set = js::new!(globalThis->Set(js::arr![1, 2]))?;
	/// assert_eq!(set.values()?.to_vec(), vec![Value::from(1), Value::from(2)]);
	/// assert!(js::obj! {}.values().is_err());
	/// # Ok(())
	/// # }
	/// ```
	fn values(&self) -> Result<List>;

	/// Returns the own enumerable string keys of `self` (`Object.keys(self)`).
	///
	/// Because JS strings are UTF-16 and Rust `String`s are UTF-8, any lone surrogate
	/// in a key is replaced with U+FFFD (`\u{FFFD}`), which will not match the original
	/// JS key when indexing back into the object.
	///
	/// # Errors
	///
	/// Returns `Err` if `self` is `null` or `undefined`, or if `Object.keys` throws.
	///
	/// # Examples
	///
	/// ```no_run
	/// use js::{Value, ValueExt as _};
	///
	/// # fn example() -> js::Result<()> {
	/// let object = js::obj! { a: 1, b: 2 };
	/// assert_eq!(object.keys()?, ["a", "b"]);
	/// assert!(Value::UNDEFINED.keys().is_err());
	/// # Ok(())
	/// # }
	/// ```
	fn keys(&self) -> Result<Vec<String>>;

	/// Returns the own enumerable `(key, value)` pairs of `self`
	/// (`Object.entries(self)`) as a `List<(String, Value)>`.
	///
	/// The `[key, value]` pairs remain in the backing JS `Array` and are converted on
	/// demand as the `List` is indexed or iterated. As with `keys`, lone UTF-16
	/// surrogates in property names are replaced with U+FFFD when converted into
	/// `String`.
	///
	/// # Errors
	///
	/// Returns `Err` if `self` is `null` or `undefined`, or if `Object.entries` throws.
	///
	/// # Examples
	///
	/// ```no_run
	/// use js::{Value, ValueExt as _};
	///
	/// # fn example() -> js::Result<()> {
	/// let object = js::obj! { a: 1 };
	/// assert_eq!(object.entries()?.to_vec(), vec![("a".to_owned(), Value::from(1))]);
	/// assert!(Value::NULL.entries().is_err());
	/// # Ok(())
	/// # }
	/// ```
	fn entries(&self) -> Result<List<(String, Value)>>;

	/// Coerces `self` to a floating-point number using JavaScript's `Number(value)`.
	///
	/// Unparseable strings and `undefined` coerce to `NaN`, not `Err`.
	///
	/// # Errors
	///
	/// Returns `Err` if `Number(self)` throws (for example on a `Symbol` or an object
	/// whose `valueOf` throws). A `BigInt` converts without error.
	///
	/// # Examples
	///
	/// ```no_run
	/// use js::{Value, ValueExt as _};
	///
	/// # fn example() -> js::Result<()> {
	/// assert_eq!(Value::from(" 12 ").number()?, 12.0);
	/// assert!(Value::UNDEFINED.number()?.is_nan());
	/// let sym = js::call!(globalThis->Symbol())?;
	/// assert!(sym.number().is_err());
	/// # Ok(())
	/// # }
	/// ```
	fn number(&self) -> Result<f64>;

	/// Coerces `self` to a signed 32-bit integer using WebIDL `long` (`ToInt32`)
	/// wrapping rules.
	///
	/// First converts `self` with `Number(self)`, then truncates toward zero and wraps
	/// modulo `2^32` into `i32::MIN..=i32::MAX`. `NaN` and `±Infinity` become `0`.
	///
	/// # Errors
	///
	/// Returns `Err` if `self.number()` throws (for example on a `Symbol`).
	///
	/// # Examples
	///
	/// ```no_run
	/// use js::{Value, ValueExt as _};
	///
	/// # fn example() -> js::Result<()> {
	/// assert_eq!(Value::from(1.9).int()?, 1);
	/// assert_eq!(Value::from(f64::NAN).int()?, 0);
	/// let sym = js::call!(globalThis->Symbol())?;
	/// assert!(sym.int().is_err());
	/// # Ok(())
	/// # }
	/// ```
	fn int(&self) -> Result<i32>;

	/// Extracts a human-readable message from an error or arbitrary `Value`.
	///
	/// Returns `self` directly if it is already a string, otherwise reads
	/// `self.message` if that property is a string, and finally falls back to
	/// `String(self)` (or `Debug` formatting if `toString` throws).
	///
	/// # Examples
	///
	/// ```no_run
	/// use js::{Value, ValueExt as _};
	///
	/// assert_eq!(js::error!("failed").message(), "failed");
	/// assert_eq!(Value::from("plain").message(), "plain");
	/// ```
	fn message(&self) -> String;

	/// Reads the string `name` property of an error or object (such as `"TypeError"` or
	/// `"AbortError"`), returning `""` if missing or not a string.
	///
	/// # Examples
	///
	/// ```no_run
	/// use js::{Value, ValueExt as _};
	///
	/// let err = js::new!(globalThis->TypeError("bad")).unwrap();
	/// assert_eq!(err.name(), "TypeError");
	/// assert_eq!(Value::NULL.name(), "");
	/// ```
	fn name(&self) -> String;
}

impl ValueExt for Value {
	fn present(self) -> Option<Value> {
		(!self.is_null_or_undefined()).then_some(self)
	}

	fn text(&self) -> String {
		self.as_string().unwrap_or_default()
	}

	fn expect_text(&self, what: &str) -> Result<String> {
		self.as_string().ok_or_else(|| crate::error!("{what} must be a string"))
	}

	fn to_list(&self) -> Result<List> {
		crate::r#macro::array_like(self).map(List::from_array)
	}

	fn values(&self) -> Result<List> {
		crate::r#macro::spread_iterable(self).map(List::from_array)
	}

	fn keys(&self) -> Result<Vec<String>> {
		let keys = crate::call!(globalThis->Object.keys(self))?;
		Ok(keys.unchecked_into::<Array>().iter().map(|key| key.text()).collect())
	}

	fn entries(&self) -> Result<List<(String, Value)>> {
		let entries = crate::call!(globalThis->Object.entries(self))?;
		Ok(List::from_array(entries.unchecked_into()))
	}

	fn number(&self) -> Result<f64> {
		crate::r#macro::number(self)
	}

	fn int(&self) -> Result<i32> {
		Ok(to_long(self.number()?))
	}

	fn message(&self) -> String {
		if let Some(text) = self.as_string() {
			return text;
		}
		if let Some(text) = crate::get!(self->message).ok().and_then(|m| m.as_string()) {
			return text;
		}
		crate::r#macro::describe(self)
	}

	fn name(&self) -> String {
		crate::get!(self->name).map(|name| name.text()).unwrap_or_default()
	}
}

// Implements ECMAScript `ToInt32` / WebIDL `long`: non-finite values map to 0, and
// finite values are truncated and wrapped modulo 2^32 into signed 32-bit range.
fn to_long(number: f64) -> i32 {
	if !number.is_finite() {
		return 0;
	}
	const MODULUS: f64 = 4_294_967_296.0;
	let wrapped = number.trunc().rem_euclid(MODULUS);
	let wrapped = if wrapped >= MODULUS / 2.0 { wrapped - MODULUS } else { wrapped };
	wrapped as i32
}

/// A JavaScript argument list backed by a `js_sys::Array`.
///
/// Used by `func!` to bind positional and `..rest` parameters, and by `call!` / `new!`
/// when spreading `..args`. Cloning an `Args` makes a shallow copy of the underlying JS
/// `Array`.
///
/// # Examples
///
/// ```no_run
/// use js::{Args, Value};
///
/// let args = Args::new([Value::from(1), Value::from(2)]);
/// assert_eq!(args.len(), 2);
/// assert_eq!(args.get(0), 1);
/// assert!(args.get(5).is_undefined());
/// ```
#[derive(Debug)]
pub struct Args(pub(crate) Array);

impl Args {
	/// Collects an iterator of `Value`s into a new `Args`.
	pub fn new(values: impl IntoIterator<Item = Value>) -> Self {
		Self(values.into_iter().collect())
	}

	/// Returns the argument at `index`, or `undefined` if `index >= self.len()`.
	pub fn get(&self, index: u32) -> Value {
		self.0.get(index)
	}

	/// Sets the argument at `index` in the underlying JS `Array`.
	pub fn set(&self, index: u32, value: Value) {
		self.0.set(index, value);
	}

	/// Removes and returns the first argument, or `undefined` if empty.
	pub fn shift(&self) -> Value {
		self.0.shift()
	}

	/// Returns a new `Args` containing the arguments starting at `count` without
	/// mutating `self`.
	pub fn skip(&self, count: u32) -> Self {
		Self(self.0.slice(count, self.0.length()))
	}

	/// Returns the number of arguments.
	pub fn len(&self) -> u32 {
		self.0.length()
	}

	/// Returns `true` if there are no arguments.
	pub fn is_empty(&self) -> bool {
		self.len() == 0
	}

	/// Copies all arguments into a `Vec<Value>`.
	pub fn to_vec(&self) -> Vec<Value> {
		self.0.to_vec()
	}
}

impl Clone for Args {
	fn clone(&self) -> Self {
		Self(Array::from(&self.0))
	}
}

impl From<Args> for Value {
	fn from(args: Args) -> Self {
		args.0.into()
	}
}

/// A typed view over a JavaScript `Array` that keeps its items in JavaScript until
/// accessed.
///
/// `wasm-bindgen` gives every live `JsValue` held in Rust a slot in the WebAssembly
/// `externref` table, which browsers cap at 10,000,000 entries (and because
/// `wasm-bindgen` grows that table by doubling, the wasm instance aborts once roughly
/// 8.4 million values are live at the same time). Collecting an untrusted,
/// page-controlled list into a `Vec<Value>` could therefore abort the wasm instance,
/// whereas `List` holds only a single `Array` handle in Rust regardless of length and
/// converts elements on demand via `ListItem`.
///
/// Cloning a `List` clones the handle to the same underlying JS `Array` (unlike
/// `Args::clone`, which copies the array). `From<List<T>> for Value` unwraps the
/// underlying JS `Array`, and `TryFrom<List<T>> for [T; N]` extracts a fixed-size Rust
/// array or fails with a JS `RangeError` (`"expected N items, got M"`) when the length
/// differs.
///
/// # Examples
///
/// ```no_run
/// use js::{Value, ValueExt as _};
///
/// # fn example() -> js::Result<()> {
/// let list = js::arr!["a", "b"].to_list()?;
/// assert_eq!(list.len(), 2);
/// assert_eq!(list.get(0), Some(Value::from("a")));
/// assert!(list.get(2).is_none());
///
/// let [first, second]: [Value; 2] = list.try_into()?;
/// assert_eq!((first, second), (Value::from("a"), Value::from("b")));
/// # Ok(())
/// # }
/// ```
pub struct List<T = Value> {
	array: Array,
	item: PhantomData<fn() -> T>,
}

/// Converts a single element read from a `List` out of its raw JavaScript `Value`.
///
/// This trait is sealed via the private `sealed::Sealed` supertrait and is implemented
/// only for `Value` (identity) and `(String, Value)` (reads a two-element
/// `[key, value]` JS `Array`, extracting the key with `ValueExt::text`). Downstream
/// crates cannot implement `ListItem`, so new methods can be added to it later without
/// a breaking change.
///
/// # Examples
///
/// Implementing `ListItem` for a downstream type fails to compile because the
/// `sealed::Sealed` supertrait is private:
///
/// ```compile_fail
/// struct Item(js::Value);
///
/// impl js::ListItem for Item {
///     fn from_value(value: js::Value) -> Self {
///         Self(value)
///     }
/// }
/// ```
pub trait ListItem: sealed::Sealed + Sized {
	/// Converts a raw element `value` from the backing JS `Array` into `Self`.
	fn from_value(value: Value) -> Self;
}

// Seals `ListItem` so downstream crates cannot implement it.
mod sealed {
	pub trait Sealed {}
	impl Sealed for super::Value {}
	impl Sealed for (String, super::Value) {}
}

impl ListItem for Value {
	fn from_value(value: Value) -> Self {
		value
	}
}

impl ListItem for (String, Value) {
	fn from_value(value: Value) -> Self {
		let pair = value.unchecked_into::<Array>();
		(pair.get(0).text(), pair.get(1))
	}
}

impl<T> List<T> {
	pub(crate) fn from_array(array: Array) -> Self {
		Self { array, item: PhantomData }
	}

	/// Returns the number of items in the underlying JS `Array`.
	pub fn len(&self) -> u32 {
		self.array.length()
	}

	/// Returns `true` if the list contains no items.
	pub fn is_empty(&self) -> bool {
		self.len() == 0
	}
}

impl<T: ListItem> List<T> {
	/// Returns the item at `index`, or `None` if `index >= self.len()`.
	///
	/// An in-bounds element whose JS value is `undefined` still returns `Some`.
	pub fn get(&self, index: u32) -> Option<T> {
		(index < self.len()).then(|| T::from_value(self.array.get(index)))
	}

	/// Returns a double-ended iterator that reads and converts one item at a time from
	/// the underlying JS `Array`.
	pub fn iter(&self) -> ListIter<T> {
		ListIter::new(self.array.clone())
	}

	/// Copies all items from the underlying JS `Array` into a Rust `Vec<T>`.
	///
	/// Because every `Value` in the returned `Vec` occupies its own slot in
	/// `wasm-bindgen`'s `externref` table (which browsers cap at 10,000,000 entries,
	/// aborting the wasm instance once roughly 8.4 million values are live), avoid
	/// calling `to_vec` on untrusted or unbounded lists; iterate with `List::iter` or
	/// index with `List::get` instead.
	pub fn to_vec(&self) -> Vec<T> {
		self.iter().collect()
	}
}

impl<T> Clone for List<T> {
	fn clone(&self) -> Self {
		Self::from_array(self.array.clone())
	}
}

impl<T> std::fmt::Debug for List<T> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_tuple("List").field(&self.array).finish()
	}
}

impl<T> From<List<T>> for Value {
	fn from(list: List<T>) -> Self {
		list.array.into()
	}
}

impl<T: ListItem> IntoIterator for List<T> {
	type Item = T;
	type IntoIter = ListIter<T>;

	fn into_iter(self) -> ListIter<T> {
		ListIter::new(self.array)
	}
}

impl<T: ListItem> IntoIterator for &List<T> {
	type Item = T;
	type IntoIter = ListIter<T>;

	fn into_iter(self) -> ListIter<T> {
		self.iter()
	}
}

impl<T: ListItem, const N: usize> TryFrom<List<T>> for [T; N] {
	type Error = Value;

	fn try_from(list: List<T>) -> Result<Self> {
		if list.len() as usize != N {
			let message = format!("expected {N} items, got {}", list.len());
			return Err(js_sys::RangeError::new(&message).into());
		}
		Ok(std::array::from_fn(|index| T::from_value(list.array.get(index as u32))))
	}
}

/// An iterator over the items of a `List`, created by `List::iter` or `IntoIterator`.
///
/// `ListIter` is double-ended, exact-size, and fused. It reads one element at a time
/// from the backing JS `Array`, so only the current item's `Value` is brought into
/// Rust on each step.
pub struct ListIter<T> {
	array: Array,
	front: u32,
	back: u32,
	item: PhantomData<fn() -> T>,
}

impl<T> ListIter<T> {
	fn new(array: Array) -> Self {
		let back = array.length();
		Self { array, front: 0, back, item: PhantomData }
	}
}

impl<T: ListItem> Iterator for ListIter<T> {
	type Item = T;

	fn next(&mut self) -> Option<T> {
		if self.front == self.back {
			return None;
		}
		let item = T::from_value(self.array.get(self.front));
		self.front += 1;
		Some(item)
	}

	fn size_hint(&self) -> (usize, Option<usize>) {
		let remaining = (self.back - self.front) as usize;
		(remaining, Some(remaining))
	}
}

impl<T: ListItem> DoubleEndedIterator for ListIter<T> {
	fn next_back(&mut self) -> Option<T> {
		if self.front == self.back {
			return None;
		}
		self.back -= 1;
		Some(T::from_value(self.array.get(self.back)))
	}
}

impl<T: ListItem> ExactSizeIterator for ListIter<T> {}

impl<T: ListItem> std::iter::FusedIterator for ListIter<T> {}

impl<T> std::fmt::Debug for ListIter<T> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("ListIter")
			.field("front", &self.front)
			.field("back", &self.back)
			.finish()
	}
}

/// The JavaScript `typeof` classification of a `Value`.
///
/// Returned by `type_of!` and `Type::of`. Note that `typeof null` is `"object"` in
/// JavaScript, so `Value::NULL` maps to `Type::Object`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Type {
	/// `"undefined"`
	Undefined,
	/// `"object"` (including `null`)
	Object,
	/// `"boolean"`
	Boolean,
	/// `"number"`
	Number,
	/// `"bigint"`
	BigInt,
	/// `"string"`
	String,
	/// `"symbol"`
	Symbol,
	/// `"function"`
	Function,
}

impl Type {
	/// Inspects `value` with JavaScript `typeof` and returns its `Type`.
	///
	/// # Examples
	///
	/// ```no_run
	/// use js::{Type, Value};
	///
	/// assert_eq!(Type::of(&Value::UNDEFINED), Type::Undefined);
	/// assert_eq!(Type::of(&Value::NULL), Type::Object);
	/// assert_eq!(Type::of(&Value::from("hi")), Type::String);
	/// ```
	pub fn of(value: &Value) -> Self {
		match value.js_typeof().as_string().as_deref() {
			Some("undefined") => Self::Undefined,
			Some("boolean") => Self::Boolean,
			Some("number") => Self::Number,
			Some("bigint") => Self::BigInt,
			Some("string") => Self::String,
			Some("symbol") => Self::Symbol,
			Some("function") => Self::Function,
			_ => Self::Object,
		}
	}

	/// Returns the lowercase string produced by JavaScript `typeof`.
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Undefined => "undefined",
			Self::Object => "object",
			Self::Boolean => "boolean",
			Self::Number => "number",
			Self::BigInt => "bigint",
			Self::String => "string",
			Self::Symbol => "symbol",
			Self::Function => "function",
		}
	}
}

impl std::fmt::Display for Type {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str(self.as_str())
	}
}

/// Creates a bare dictionary object with a `null` prototype (`Object.create(null)`).
///
/// Because it has no prototype chain, properties like `toString` and `valueOf` are
/// absent unless explicitly set.
///
/// # Examples
///
/// ```no_run
/// # fn example() -> js::Result<()> {
/// let map = js::dict();
/// assert!(!js::has!(map->toString)?);
/// # Ok(())
/// # }
/// ```
pub fn dict() -> Value {
	Object::<Value>::create(Value::NULL.unchecked_ref()).into()
}

/// Builds a plain JavaScript object from an iterator of `(key, value)` pairs via
/// `Object.fromEntries`.
///
/// Because `Object.fromEntries` defines own data properties rather than invoking
/// setters on `Object.prototype`, a `"__proto__"` key is stored as a normal own
/// property without modifying the object's prototype.
///
/// # Examples
///
/// ```no_run
/// # fn example() -> js::Result<()> {
/// let obj = js::object([("a", 1), ("b", 2)]);
/// assert_eq!(js::get!(obj->a)?, 1);
/// # Ok(())
/// # }
/// ```
pub fn object<K, V>(entries: impl IntoIterator<Item = (K, V)>) -> Value
where
	K: Into<Value>,
	V: Into<Value>,
{
	let entries = entries
		.into_iter()
		.map(|(key, value)| Array::of2(&key.into(), &value.into()))
		.collect::<Array>();
	Object::from_entries(&entries)
		.map_or_else(|_| crate::r#macro::empty_object(), Into::into)
}

/// Serializes a Rust value to a JavaScript `Value` by encoding to JSON and parsing with
/// `JSON.parse`.
///
/// Because the value passes through JSON:
/// - Rust maps serialize as plain JS objects (not `Map` instances), and map keys that
///   cannot be serialized as JSON strings fail.
/// - 64-bit integers outside `±(2^53 - 1)` (`Number.MAX_SAFE_INTEGER`) are parsed as
///   IEEE-754 `f64` numbers in JS and lose low-bit precision.
///
/// # Errors
///
/// Returns `Err` if `serde_json::to_string` fails (for example on a map with non-string
/// keys or a custom `Serialize` implementation that fails).
///
/// # Examples
///
/// ```no_run
/// use std::collections::BTreeMap;
/// # fn example() -> js::Result<()> {
/// let value = js::serialize(&vec![1, 2, 3])?;
/// assert_eq!(js::get!(value->[1])?, 2);
///
/// let bad_map = BTreeMap::from([((1, 2), "v")]);
/// assert!(js::serialize(&bad_map).is_err());
/// # Ok(())
/// # }
/// ```
pub fn serialize<T: Serialize + ?Sized>(value: &T) -> Result {
	let json = serde_json::to_string(value).map_err(|error| crate::error!("{error}"))?;
	JSON::parse(&json)
}

/// Deserializes a Rust value `T` from a JavaScript `Value` via `JSON.stringify` and
/// `serde_json::from_str`.
///
/// Going through `JSON.stringify` guarantees that cyclic object graphs fail cleanly
/// with a JS `TypeError` instead of overflowing the wasm call stack. It also means
/// `JSON.stringify` semantics apply:
/// - Object properties with `undefined`, function, or `Symbol` values are omitted, and
///   those values inside arrays become `null`.
/// - Top-level `undefined`, functions, or symbols produce `undefined` from
///   `JSON.stringify`, which this function treats as `"null"` (deserializing into
///   `None` for `Option<T>`).
/// - Objects with a `toJSON()` method (such as `Date`) serialize through `toJSON()`.
///
/// # Errors
///
/// Returns `Err` if `JSON.stringify` throws (such as a `TypeError` on cyclic objects
/// or `BigInt` values) or if `serde_json` fails to deserialize the resulting JSON into
/// `T`.
///
/// # Examples
///
/// ```no_run
/// # fn example() -> js::Result<()> {
/// let nums: Vec<i32> = js::deserialize(&js::arr![1, 2, 3])?;
/// assert_eq!(nums, [1, 2, 3]);
///
/// let cyclic = js::obj! {};
/// js::set!(cyclic->self_ref = cyclic)?;
/// assert!(js::deserialize::<serde_json::Value>(&cyclic).is_err());
/// # Ok(())
/// # }
/// ```
pub fn deserialize<T: DeserializeOwned>(value: &Value) -> Result<T> {
	let json = JSON::stringify(value)?.as_string();
	let json = json.as_deref().unwrap_or("null");
	serde_json::from_str(json).map_err(|error| crate::error!("{error}"))
}

/// Reads `target[key]` via `Reflect.get` and, if it is a method (a function without an
/// own `prototype` property), returns it bound to `target` via `.bind(target)`.
///
/// Non-functions and constructors (functions that have an own `prototype` property,
/// such as `Object`, `Array`, or `Promise`) are returned unbound so static properties
/// and `new` construction remain intact. Note that closures created with
/// `func!(|this, ...| ...)` wrap an object method and do not have an own `prototype`,
/// so `get_bound` binds their `this` to `target` as well.
///
/// # Errors
///
/// Returns `Err` if `target` is not an object, if reading `target[key]` or checking
/// `prototype` throws, or if `.bind(target)` throws.
///
/// # Examples
///
/// ```no_run
/// # fn example() -> js::Result<()> {
/// let map = js::new!(globalThis->Map())?;
/// js::call!(map->set("k", "v"))?;
/// let get = js::get_bound(&map, &"get".into())?;
/// assert_eq!(js::call!(get->("k"))?, "v");
///
/// assert!(js::get_bound(&js::Value::NULL, &"k".into()).is_err());
/// # Ok(())
/// # }
/// ```
pub fn get_bound(target: &Value, key: &Value) -> Result {
	let value = Reflect::get(target, key)?;
	if !value.is_function() || crate::own!(value->prototype)? {
		return Ok(value);
	}
	crate::call!(value->bind(target))
}
