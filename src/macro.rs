//! Macro definitions and the runtime helpers their expansions call.

use std::future::Future;

use js_sys::{Array, Function, Object, Reflect, TypeError};
use wasm_bindgen::convert::ReturnWasmAbi;
use wasm_bindgen::prelude::{Closure, wasm_bindgen};
use wasm_bindgen::{JsCast, JsValue};

use crate::{Args, JsResult, promise};

/// Reads a property or expression chain across the JavaScript boundary.
///
/// The path starts with `globalThis->` or `expr->`, followed by any mix of `.key`,
/// `[key]`, `?.key`, `?.(args)`, and `(args)` steps. Raw identifiers such as `r#type`
/// have their `r#` prefix stripped so they access `"type"`.
///
/// Optional chaining (`?.`) short-circuits the remainder of the path to `Ok(undefined)`
/// when the receiver is `null` or `undefined`.
///
/// # Errors
///
/// Returns `Err` if reading a property along the path throws (for example, reading a
/// property on `null` or `undefined` without `?.`, or when a getter throws).
///
/// # Examples
///
/// ```no_run
/// # fn example() -> js::Result<()> {
/// let user = js::obj! { name: "Ada", r#type: "admin" };
/// assert_eq!(js::get!(user->name)?, "Ada");
/// assert_eq!(js::get!(user->r#type)?, "admin");
/// assert!(js::get!(user->profile?.bio)?.is_undefined());
/// assert!(js::get!(user->profile.bio).is_err());
/// # Ok(())
/// # }
/// ```
#[macro_export]
macro_rules! get {
	($($path:tt)+) => { $crate::__js_scan!(@get [] $($path)+) };
}

/// Calls a JavaScript function or method at the end of a path.
///
/// When the final step is a property (`obj->method(a, b)` or `obj->["method"](a, b)`),
/// the function is invoked with `obj` as `this`. When the root itself is called
/// (`fn_val->(a, b)`), `this` is `undefined`. Call arguments may be a comma-separated
/// list of expressions or a single `..rest` spread of an `Args`, slice, array, or
/// `Vec<Value>`.
///
/// A path passed to `call!` must end with `(args)` or `?.(args)`; ending with a
/// property read is a compile error. If an optional chain (`?.`) short-circuits along
/// the path, `call!` resolves to `Ok(undefined)` without invoking the call.
///
/// # Errors
///
/// Returns `Err` with a `TypeError` if the target is not a function (including when
/// `f?.()` finds a non-nullish value that is not callable), or returns the thrown JS
/// value if evaluating the path or the function throws.
///
/// # Examples
///
/// ```no_run
/// # fn example() -> js::Result<()> {
/// let map = js::new!(globalThis->Map())?;
/// js::call!(map->set("key", 42))?;
/// assert_eq!(js::call!(map->get("key"))?, 42);
/// assert!(js::call!(map->missing?.())?.is_undefined());
/// assert!(js::call!(map->size()).is_err());
/// # Ok(())
/// # }
/// ```
#[macro_export]
macro_rules! call {
	($($path:tt)+) => { $crate::__js_scan!(@call [] $($path)+) };
}

/// Assigns a value to the property at the end of a JavaScript path.
///
/// Intermediate steps are read as in `get!`, and the final `.key` or `[key]` is
/// written using `Reflect.set`. Using `?.` anywhere in a `set!` path is a compile
/// error, matching JavaScript syntax rules.
///
/// # Errors
///
/// Returns `Err` if walking the path throws, or with a `TypeError` if `Reflect.set`
/// returns `false` (for example on a frozen object or a read-only property) or the
/// target is not an object.
///
/// # Examples
///
/// ```no_run
/// # fn example() -> js::Result<()> {
/// let object = js::obj! { count: 1 };
/// js::set!(object->count = 2)?;
/// assert_eq!(js::get!(object->count)?, 2);
///
/// let frozen = js::call!(globalThis->Object.freeze(object))?;
/// assert!(js::set!(frozen->count = 3).is_err());
/// # Ok(())
/// # }
/// ```
#[macro_export]
macro_rules! set {
	($($path:tt)+) => { $crate::__js_scan!(@set [] $($path)+) };
}

/// Checks whether an object or its prototype chain has a property (`key in target`).
///
/// Walks the path up to the final `.key` or `[key]` and queries `Reflect.has`. If an
/// optional chain (`?.`) short-circuits along the path, returns `Ok(false)`.
///
/// # Errors
///
/// Returns `Err` if walking the path throws or if the final target is not an object
/// (`Reflect.has` throws a `TypeError` on primitives).
///
/// # Examples
///
/// ```no_run
/// # fn example() -> js::Result<()> {
/// let object = js::obj! { a: 1 };
/// assert!(js::has!(object->a)?);
/// assert!(js::has!(object->toString)?);
/// assert!(!js::has!(object->missing?.deep)?);
/// assert!(js::has!(js::Value::NULL->a).is_err());
/// # Ok(())
/// # }
/// ```
#[macro_export]
macro_rules! has {
	($($path:tt)+) => { $crate::__js_scan!(@has [] $($path)+) };
}

/// Checks whether an object has an own property (`Object.hasOwn(target, key)`).
///
/// Unlike `has!`, inherited properties on the prototype chain return `Ok(false)`. If
/// an optional chain (`?.`) short-circuits along the path, returns `Ok(false)`.
///
/// # Errors
///
/// Returns `Err` if walking the path throws or if the final target is `null` or
/// `undefined` without `?.`.
///
/// # Examples
///
/// ```no_run
/// # fn example() -> js::Result<()> {
/// let object = js::obj! { a: 1 };
/// assert!(js::own!(object->a)?);
/// assert!(!js::own!(object->toString)?);
/// assert!(!js::own!(object->missing?.a)?);
/// assert!(js::own!(js::Value::NULL->a).is_err());
/// # Ok(())
/// # }
/// ```
#[macro_export]
macro_rules! own {
	($($path:tt)+) => { $crate::__js_scan!(@own [] $($path)+) };
}

/// Defines or modifies a property on an object using `Reflect.defineProperty`.
///
/// The second argument is an object-literal descriptor (`{ value: ..., writable: ...,
/// enumerable: ..., configurable: ..., get: ..., set: ... }`). Using `?.` in the path
/// is a compile error.
///
/// # Errors
///
/// Returns `Err` if walking the path throws, if the target is not an object, or if
/// `Reflect.defineProperty` returns `false` (for example on a non-extensible object or
/// non-configurable property).
///
/// # Examples
///
/// ```no_run
/// # fn example() -> js::Result<()> {
/// let object = js::obj! {};
/// js::def!(object->answer, { value: 42, enumerable: true })?;
/// assert_eq!(js::get!(object->answer)?, 42);
/// assert!(js::set!(object->answer = 0).is_err());
/// # Ok(())
/// # }
/// ```
#[macro_export]
macro_rules! def {
	($($path:tt)+) => { $crate::__js_scan!(@def [] $($path)+) };
}

/// Deletes a property from an object (`delete target.key`).
///
/// If an optional chain (`?.`) short-circuits along the path, returns `Ok(())`,
/// matching JavaScript's `delete a?.b`.
///
/// # Errors
///
/// Returns `Err` if walking the path throws, if the target is not an object, or if
/// `Reflect.deleteProperty` returns `false` (such as when deleting a non-configurable
/// property).
///
/// # Examples
///
/// ```no_run
/// # fn example() -> js::Result<()> {
/// let object = js::obj! { temp: 1 };
/// js::del!(object->temp)?;
/// assert!(!js::own!(object->temp)?);
///
/// js::def!(object->locked, { value: 1 })?;
/// assert!(js::del!(object->locked).is_err());
/// # Ok(())
/// # }
/// ```
#[macro_export]
macro_rules! del {
	($($path:tt)+) => { $crate::__js_scan!(@del [] $($path)+) };
}

/// Reads a property once, transforms it with a closure, writes the result back, and
/// returns the new `Value`.
///
/// Intermediate path steps (including method calls) are evaluated only once to locate
/// the receiver object before reading and writing the final key. Using `?.` in the
/// path is a compile error.
///
/// # Errors
///
/// Returns `Err` if evaluating the path, reading the old value, or assigning the new
/// value fails (including `TypeError` on refused strict-mode writes).
///
/// # Examples
///
/// ```no_run
/// # fn example() -> js::Result<()> {
/// let stats = js::obj! { hits: 10 };
/// let updated = js::update!(stats->hits, |old| old.as_f64().unwrap_or(0.0) + 1.0)?;
/// assert_eq!(updated, 11);
/// assert_eq!(js::get!(stats->hits)?, 11);
///
/// let frozen = js::call!(globalThis->Object.freeze(stats))?;
/// assert!(js::update!(frozen->hits, |_| 0).is_err());
/// # Ok(())
/// # }
/// ```
#[macro_export]
macro_rules! update {
	($($path:tt)+) => { $crate::__js_scan!(@update [] $($path)+) };
}

/// Constructs an instance with `Reflect.construct` (`new Path(args)`).
///
/// The path must end with `(args)`, either after a property lookup
/// (`globalThis->Map()`) or directly on a constructor value (`ctor->(a, b)`). Using
/// `?.` in the path is a compile error.
///
/// # Errors
///
/// Returns `Err` if evaluating the path throws, if the target is not a constructor, or
/// if the constructor itself throws.
///
/// # Examples
///
/// ```no_run
/// # fn example() -> js::Result<()> {
/// let set = js::new!(globalThis->Set())?;
/// assert!(js::is!(set, Set));
/// assert!(js::new!(globalThis->Math.abs()).is_err());
/// # Ok(())
/// # }
/// ```
#[macro_export]
macro_rules! new {
	($($path:tt)+) => { $crate::__js_scan!(@new [] $($path)+) };
}

/// Evaluates a JavaScript path and awaits the resulting `Promise` or value.
///
/// Non-Promise values resolve immediately via `Promise.resolve`.
///
/// # Errors
///
/// The returned future resolves to `Err` if evaluating the path throws synchronously
/// or if the awaited `Promise` rejects.
///
/// # Examples
///
/// ```no_run
/// # async fn example() -> js::Result<()> {
/// let value = js::wait!(globalThis->Promise.resolve(42)).await?;
/// assert_eq!(value, 42);
/// assert!(js::wait!(globalThis->Promise.reject("boom")).await.is_err());
/// # Ok(())
/// # }
/// ```
#[macro_export]
macro_rules! wait {
	($($path:tt)+) => {
		$crate::r#macro::wait($crate::__js_scan!(@get [] $($path)+))
	};
}

/// Tests whether a value is an instance of a JavaScript constructor (`instanceof`).
///
/// Accepts either a bare global class identifier (`is!(val, HTMLElement)`, which looks
/// up `globalThis->HTMLElement`) or a full path (`is!(val, frame->HTMLElement)`).
/// Returns `false` instead of failing if the constructor cannot be found or if
/// `instanceof` throws.
///
/// # Examples
///
/// ```no_run
/// let map = js::new!(globalThis->Map()).unwrap();
/// assert!(js::is!(map, Map));
/// assert!(js::is!(map, globalThis->Object));
/// assert!(!js::is!(map, Array));
/// assert!(!js::is!(map, MissingGlobalClass));
/// ```
#[macro_export]
macro_rules! is {
	// Bare identifier shorthand looks up the constructor on `globalThis`. Must precede
	// the general path arm so a single identifier does not fail the `->` scan.
	($value:expr, $class:ident) => {
		$crate::is!($value, globalThis->$class)
	};
	($value:expr, $($path:tt)+) => {
		$crate::r#macro::instance_of(&$value, $crate::__js_scan!(@get [] $($path)+))
	};
}

/// Returns the JavaScript `typeof` classification of the value at a path as a `Type`.
///
/// # Errors
///
/// Returns `Err` if evaluating the path throws.
///
/// # Examples
///
/// ```no_run
/// # fn example() -> js::Result<()> {
/// assert_eq!(js::type_of!(globalThis->Math.PI)?, js::Type::Number);
/// assert!(js::type_of!(js::Value::NULL->missing).is_err());
/// # Ok(())
/// # }
/// ```
#[macro_export]
macro_rules! type_of {
	($($path:tt)+) => {
		$crate::__js_scan!(@get [] $($path)+).map(|value| $crate::Type::of(&value))
	};
}

/// Returns the own enumerable string keys of the value at a path (`Object.keys`).
///
/// Unpaired UTF-16 surrogates in key names are replaced with U+FFFD when converted to
/// Rust `String`s.
///
/// # Errors
///
/// Returns `Err` if evaluating the path throws or if the value is `null` or
/// `undefined`.
///
/// # Examples
///
/// ```no_run
/// # fn example() -> js::Result<()> {
/// let root = js::obj! { inner: js::obj! { a: 1, b: 2 } };
/// assert_eq!(js::keys!(root->inner)?, ["a", "b"]);
/// assert!(js::keys!(js::Value::NULL->inner).is_err());
/// # Ok(())
/// # }
/// ```
#[macro_export]
macro_rules! keys {
	($($path:tt)+) => {
		$crate::__js_scan!(@get [] $($path)+)
			.and_then(|value| $crate::ValueExt::keys(&value))
	};
}

/// Returns the own enumerable `(key, value)` pairs of the value at a path
/// (`Object.entries`) as a `List<(String, Value)>`.
///
/// Pairs stay in the backing JS `Array` and are converted on demand as the `List` is
/// indexed or iterated. Unpaired UTF-16 surrogates in key names are replaced with
/// U+FFFD when converted to Rust `String`s.
///
/// # Errors
///
/// Returns `Err` if evaluating the path throws or if the value is `null` or
/// `undefined`.
///
/// # Examples
///
/// ```no_run
/// # fn example() -> js::Result<()> {
/// let root = js::obj! { inner: js::obj! { a: 1 } };
/// assert_eq!(
///     js::entries!(root->inner)?.to_vec(),
///     vec![("a".to_owned(), js::Value::from(1))],
/// );
/// assert!(js::entries!(js::Value::UNDEFINED->inner).is_err());
/// # Ok(())
/// # }
/// ```
#[macro_export]
macro_rules! entries {
	($($path:tt)+) => {
		$crate::__js_scan!(@get [] $($path)+)
			.and_then(|value| $crate::ValueExt::entries(&value))
	};
}

// Token muncher with state `@$mode [$($root)*] $($rest)*`.
//
// Accumulates tokens into `[$root]` until it encounters `->`, because a Rust expression
// cannot be matched directly before `->` in `macro_rules!`.
//
// Rule order matters:
// 1-2. `-> ( ... )` and `-> [ ... ]` must match before the general `-> $rest` rule so
//      that direct calls (`fn->()`) and bracket indexing (`obj->["k"]`) do not get a
//      synthetic leading `.` inserted before them.
// 3.   `-> $rest` inserts a leading `.` so the first identifier step is parsed uniform
//      with subsequent `.ident` steps in `__js_walk!`.
// 4-5. Munch one token into `$root`, or emit a compile error if tokens run out without
//      seeing `->`.
#[doc(hidden)]
#[macro_export]
macro_rules! __js_scan {
	(@$mode:ident [$($root:tt)+] -> ( $($args:tt)* ) $($rest:tt)*) => {
		$crate::__js_walk!(@$mode [$crate::__js_base!($($root)+)] ( $($args)* ) $($rest)*)
	};
	(@$mode:ident [$($root:tt)+] -> [ $($key:tt)* ] $($rest:tt)*) => {
		$crate::__js_walk!(@$mode [$crate::__js_base!($($root)+)] [ $($key)* ] $($rest)*)
	};
	(@$mode:ident [$($root:tt)+] -> $($rest:tt)+) => {
		$crate::__js_walk!(@$mode [$crate::__js_base!($($root)+)] . $($rest)+)
	};
	(@$mode:ident [$($root:tt)*] $next:tt $($rest:tt)*) => {
		$crate::__js_scan!(@$mode [$($root)* $next] $($rest)*)
	};
	(@$mode:ident [$($root:tt)*]) => {
		::core::compile_error!("JS path must contain `->`")
	};
}

// Evaluates the root before `->` into `Result<JsValue, JsValue>`.
// `globalThis` must precede the general `$($root)+` arm so it resolves to the JS
// global object rather than a Rust variable named `globalThis`.
#[doc(hidden)]
#[macro_export]
macro_rules! __js_base {
	(globalThis) => { $crate::__js_ok!($crate::r#macro::global()) };
	($($root:tt)+) => {
		$crate::__js_ok!(::core::clone::Clone::clone(
			::core::convert::AsRef::<$crate::wasm_bindgen::JsValue>::as_ref(&($($root)+)),
		))
	};
}

// Wraps a `JsValue` expression in `Result::<JsValue, JsValue>::Ok` with full type
// annotations so type inference inside chained `.and_then(...)` closures never stalls.
#[doc(hidden)]
#[macro_export]
macro_rules! __js_ok {
	($value:expr) => {
		::core::result::Result::<
			$crate::wasm_bindgen::JsValue,
			$crate::wasm_bindgen::JsValue,
		>::Ok($value)
	};
}

// Step-by-step path walker with state `@$mode [$cur:expr] $($rest)*`, where `$cur` is
// an expression of type `Result<JsValue, JsValue>` holding the current receiver.
//
// `.ident` and `[expr]` steps are first normalized into `{ key_expr }` so that every
// terminal and intermediate operation only needs to handle `{ $key }` once.
//
// Rule order is critical:
// 1-2. Normalize leading `.ident` and `[expr]` into `{ $key }`.
// 3-10. Terminal operations (`@has`, `@own`, `@def`, `@set`, `@del`, `@update`, and
//      both `@new` forms) match when only the final key (and operation payload)
//      remains.
// 11.  `@get [$cur]` with no remaining tokens finishes a read.
// 12-16. `@call` terminal rules: a call at the very end delegates to `@get`, while a
//      path ending in an uncalled property (`{ $key }` or `?. $key`) triggers a
//      compile error instead of silently reading.
// 17.  Intermediate/final optional method call `{ $key } ?. ( ... )` checks whether
//      the looked-up property is nullish before calling it with `$cur` as `this`, and
//      short-circuits the entire remaining path via `__js_skip!` if so.
// 18.  Regular method call `{ $key } ( ... )` must precede plain property read (19) so
//      parentheses after a key are treated as a method call with `this`, not a property
//      read followed by an unbound function call.
// 19.  Intermediate property read `{ $key }` advances `$cur` and continues walking.
// 20.  Optional property chain `?. $key` short-circuits the rest of the path via
//      `__js_skip!` when `$cur` is `null` or `undefined`.
// 21.  Direct call `( ... )` invokes `$cur` with `this = undefined`.
#[doc(hidden)]
#[macro_export]
macro_rules! __js_walk {
	(@$mode:ident [$cur:expr] . $key:ident $($rest:tt)*) => {
		$crate::__js_walk!(@$mode [$cur] { $crate::__js_key!($key) } $($rest)*)
	};
	(@$mode:ident [$cur:expr] [ $key:expr ] $($rest:tt)*) => {
		$crate::__js_walk!(@$mode [$cur] { $crate::r#macro::arg(&$key) } $($rest)*)
	};
	(@has [$cur:expr] { $key:expr }) => {
		$cur.and_then(|this| $crate::r#macro::has(&this, &$key))
	};
	(@own [$cur:expr] { $key:expr }) => {
		$cur.and_then(|this| $crate::r#macro::own(&this, &$key))
	};
	(@def [$cur:expr] { $key:expr } , { $($name:ident : $value:expr),* $(,)? }) => {
		$cur.and_then(|this| {
			let descriptor = $crate::obj! { $($name: $value),* };
			$crate::r#macro::define_property(&this, &$key, &descriptor)
		})
	};
	(@set [$cur:expr] { $key:expr } = $value:expr) => {
		$cur.and_then(|this| {
			let value = $crate::r#macro::arg(&$value);
			$crate::r#macro::set(&this, &$key, &value)
		})
	};
	(@del [$cur:expr] { $key:expr }) => {
		$cur.and_then(|this| $crate::r#macro::delete_property(&this, &$key))
	};
	(@update [$cur:expr] { $key:expr } , $f:expr $(,)?) => {
		$cur.and_then(|this| $crate::r#macro::update(&this, &$key, $f))
	};
	(@new [$cur:expr] { $key:expr } ( $($args:tt)* )) => {
		$cur.and_then(|this| {
			let constructor = $crate::r#macro::get(&this, &$key)?;
			$crate::r#macro::construct(&constructor, &$crate::__js_args!($($args)*))
		})
	};
	(@new [$cur:expr] ( $($args:tt)* )) => {
		$cur.and_then(|constructor| {
			$crate::r#macro::construct(&constructor, &$crate::__js_args!($($args)*))
		})
	};
	(@get [$cur:expr]) => { $cur };
	(@call [$cur:expr] { $key:expr } ? . ( $($args:tt)* )) => {
		$crate::__js_walk!(@get [$cur] { $key } ? . ( $($args)* ))
	};
	(@call [$cur:expr] { $key:expr } ( $($args:tt)* )) => {
		$crate::__js_walk!(@get [$cur] { $key } ( $($args)* ))
	};
	(@call [$cur:expr] ( $($args:tt)* )) => {
		$crate::__js_walk!(@get [$cur] ( $($args)* ))
	};
	(@call [$cur:expr] { $key:expr }) => {
		::core::compile_error!("`call!` path must end with a call such as `->f()`; use `get!` to read")
	};
	(@call [$cur:expr] ? . $key:ident) => {
		::core::compile_error!("`call!` path must end with a call such as `->f()`; use `get!` to read")
	};
	(@$mode:ident [$cur:expr] { $key:expr } ? . ( $($args:tt)* ) $($rest:tt)*) => {
		$cur.and_then(|this| {
			let key = $key;
			let function = $crate::r#macro::get(&this, &key)?;
			if function.is_null_or_undefined() {
				return $crate::__js_skip!(@$mode);
			}
			let args = $crate::__js_args!($($args)*);
			let name = ::core::option::Option::Some(&key);
			let result = $crate::r#macro::call(&this, &function, name, &args);
			$crate::__js_walk!(@$mode [result] $($rest)*)
		})
	};
	(@$mode:ident [$cur:expr] { $key:expr } ( $($args:tt)* ) $($rest:tt)*) => {
		$crate::__js_walk!(@$mode [$cur.and_then(|this| {
			let key = $key;
			let function = $crate::r#macro::get(&this, &key)?;
			let args = $crate::__js_args!($($args)*);
			let name = ::core::option::Option::Some(&key);
			$crate::r#macro::call(&this, &function, name, &args)
		})] $($rest)*)
	};
	(@$mode:ident [$cur:expr] { $key:expr } $($rest:tt)*) => {
		$crate::__js_walk!(@$mode [$cur.and_then(|this| {
			$crate::r#macro::get(&this, &$key)
		})] $($rest)*)
	};
	(@$mode:ident [$cur:expr] ? . $key:ident $($rest:tt)*) => {
		$cur.and_then(|this| {
			if this.is_null_or_undefined() {
				return $crate::__js_skip!(@$mode);
			}
			let key = $crate::__js_key!($key);
			$crate::__js_walk!(@$mode [$crate::__js_ok!(this)] { key } $($rest)*)
		})
	};
	(@$mode:ident [$cur:expr] ( $($args:tt)* ) $($rest:tt)*) => {
		$crate::__js_walk!(@$mode [$cur.and_then(|function| {
			$crate::r#macro::call(
				&$crate::wasm_bindgen::JsValue::UNDEFINED,
				&function,
				::core::option::Option::None,
				&$crate::__js_args!($($args)*),
			)
		})] $($rest)*)
	};
}

// Short-circuit value when `?.` encounters `null` or `undefined` in `@$mode`.
// Write and construction modes (`@set`, `@def`, `@update`, `@new`) fall through to the
// catch-all arm and reject optional chaining at compile time.
#[doc(hidden)]
#[macro_export]
macro_rules! __js_skip {
	(@get) => {
		::core::result::Result::Ok($crate::wasm_bindgen::JsValue::UNDEFINED)
	};
	(@call) => {
		::core::result::Result::Ok($crate::wasm_bindgen::JsValue::UNDEFINED)
	};
	(@has) => {
		::core::result::Result::Ok(false)
	};
	(@own) => {
		::core::result::Result::Ok(false)
	};
	(@del) => {
		::core::result::Result::Ok(())
	};
	(@$mode:ident) => {
		::core::compile_error!(::core::concat!(
			"`",
			::core::stringify!($mode),
			"!` path can't contain `?.`; as in JS, optional chaining can't be written \
			 to or constructed. Read the object with `get!` first",
		))
	};
}

// Converts a property key token into a `JsValue`.
// Identifiers strip any `r#` raw prefix; literals are converted directly without
// stripping.
#[doc(hidden)]
#[macro_export]
macro_rules! __js_key {
	($key:ident) => {
		$crate::r#macro::ident_key(::core::stringify!($key))
	};
	($key:literal) => {
		$crate::wasm_bindgen::JsValue::from($key)
	};
}

// Converts call arguments `( ... )` into a slice-compatible collection of `JsValue`s.
// `..rest` must come first so it is not parsed as a single range expression.
#[doc(hidden)]
#[macro_export]
macro_rules! __js_args {
	(.. $rest:expr) => {
		($rest).to_vec()
	};
	($($arg:expr),* $(,)?) => {
		[$($crate::r#macro::arg(&$arg)),*]
	};
}

/// Returns the JavaScript `globalThis` object as a `JsValue`.
pub fn global() -> JsValue {
	js_sys::global().into()
}

/// Converts a stringified Rust identifier into a JS property key, stripping a leading
/// `r#` raw-identifier prefix if present.
pub fn ident_key(name: &'static str) -> JsValue {
	JsValue::from(name.strip_prefix("r#").unwrap_or(name))
}

/// Clones and converts a Rust value into a `JsValue` so macro arguments can be passed
/// by value without moving the caller's variable when it is `Clone`.
pub fn arg<T: Clone + Into<JsValue>>(value: &T) -> JsValue {
	value.clone().into()
}

/// Invokes `function` with `this` as the receiver and `args` via `Reflect.apply`.
///
/// If `function` is not callable, returns a `TypeError`: when `key` is `Some(key)` (a
/// method call), the error message names the property (`"<key> is not a function"`,
/// with `key` formatted via `String(key)`); when `key` is `None` (a direct call), the
/// message is `"value is not a function"`.
pub fn call(
	this: &JsValue,
	function: &JsValue,
	key: Option<&JsValue>,
	args: &[JsValue],
) -> JsResult {
	let name = || key.map_or_else(|| "value".to_owned(), describe);
	let function = callable(function, "function", name)?;
	Reflect::apply(function, this, &args.iter().collect())
}

// Casts `value` to a JS `Function` or constructs a `TypeError` using `name()` and
// `what` (`"function"` or `"constructor"`).
fn callable<'a>(
	value: &'a JsValue,
	what: &str,
	name: impl FnOnce() -> String,
) -> JsResult<&'a Function> {
	value
		.dyn_ref()
		.ok_or_else(|| TypeError::new(&format!("{} is not a {what}", name())).into())
}

/// Checks whether `key` exists in `target` or its prototype chain (`Reflect.has`).
pub fn has(target: &JsValue, key: &JsValue) -> JsResult<bool> {
	Reflect::has(target, key)
}

/// Defines a property on `target` via `Reflect.defineProperty`, returning a
/// `TypeError` when `target` is not an object or when the definition is refused.
pub fn define_property(
	target: &JsValue,
	key: &JsValue,
	descriptor: &JsValue,
) -> JsResult<()> {
	if !target.is_object() && !target.is_function() {
		return Err(TypeError::new("cannot define property on non-object").into());
	}
	let object = target.unchecked_ref::<Object>();
	if Reflect::define_property(object, key, descriptor.unchecked_ref())? {
		return Ok(());
	}
	Err(TypeError::new(&format!("cannot define property {}", describe(key))).into())
}

/// Sets `target[key] = value` via `Reflect.set`, returning a strict-mode `TypeError` if
/// the assignment returns `false`.
pub fn set(target: &JsValue, key: &JsValue, value: &JsValue) -> JsResult<()> {
	if Reflect::set(target, key, value)? {
		return Ok(());
	}
	Err(TypeError::new(&format!("cannot assign to property {}", describe(key))).into())
}

/// Deletes `target[key]` via `Reflect.deleteProperty`, returning a `TypeError` if
/// `target` is not an object or the property is non-configurable.
pub fn delete_property(target: &JsValue, key: &JsValue) -> JsResult<()> {
	if !target.is_object() && !target.is_function() {
		return Err(TypeError::new("cannot delete property on non-object").into());
	}
	if Reflect::delete_property(target.unchecked_ref::<Object>(), key)? {
		return Ok(());
	}
	Err(TypeError::new(&format!("cannot delete property {}", describe(key))).into())
}

/// Reads `target[key]`, passes it to `f`, writes the returned value back to
/// `target[key]`, and returns the new `JsValue`.
pub fn update<T: Into<JsValue>>(
	target: &JsValue,
	key: &JsValue,
	f: impl FnOnce(JsValue) -> T,
) -> JsResult {
	let value = f(get(target, key)?).into();
	set(target, key, &value)?;
	Ok(value)
}

/// Constructs a new instance from `constructor` and `args` via `Reflect.construct`.
pub fn construct(constructor: &JsValue, args: &[JsValue]) -> JsResult {
	let constructor = callable(constructor, "constructor", || "value".to_owned())?;
	Reflect::construct(constructor, &args.iter().collect())
}

/// Propagates any synchronous path error, or awaits `value` via `Promise.resolve`.
pub async fn wait(value: JsResult) -> JsResult {
	crate::resolve(value?).await
}

/// Checks `value instanceof constructor`, returning `false` if looking up `constructor`
/// or evaluating `instanceof` fails.
pub fn instance_of(value: &JsValue, constructor: JsResult) -> bool {
	constructor
		.and_then(|constructor| instance_of_raw(value, &constructor))
		.unwrap_or(false)
}

// Formats a `JsValue` via JS `String(value)`, falling back to `Debug` if `toString`
// throws (for instance on an object with a throwing `toString` or null prototype).
pub(crate) fn describe(value: &JsValue) -> String {
	string(value).unwrap_or_else(|_| format!("{value:?}"))
}

/// Builds a plain JavaScript object (`{ ... }`).
///
/// Keys can be bare identifiers (`id: 1`, or `r#type: "button"` which becomes
/// `"type"`), string or number literals (`"aria-label": "close"`), or computed
/// expressions in brackets (`[key]: value`). `..source` entries may appear at any
/// position, any number of times, to shallow-copy own enumerable properties from
/// `source`; as in JS object literals, later entries override earlier ones.
///
/// When every entry uses an identifier or literal key, `obj!` returns a plain `Value`.
/// If a `..source` spread or a `[computed]` key appears anywhere in the literal, `obj!`
/// returns `js::Result<Value>` instead, chosen at compile time from the syntax alone.
///
/// As in JavaScript, a `[computed]` key is converted to a property key before its value
/// expression is evaluated, so if key conversion throws, the value expression is never
/// run. Strings, numbers, and symbols are used directly; other values are converted via
/// JS (`Reflect.ownKeys({ [key]: undefined })[0]`).
///
/// Properties from both explicit keys and `..source` spreads are installed with
/// `Object.defineProperty`, so `"__proto__"` is always defined as an ordinary own data
/// property and never mutates the object's prototype.
///
/// # Errors
///
/// When returning `js::Result<Value>`, returns `Err` with the thrown JS value if
/// spreading `..source` throws (for example in a getter or `Proxy` `ownKeys`/`get`
/// trap) or if converting a `[computed]` key throws (in `toString`, `valueOf`, or
/// `[Symbol.toPrimitive]`). Catching the exception as `Err` prevents it from unwinding
/// across Rust stack frames and skipping Rust destructors.
///
/// # Examples
///
/// ```no_run
/// # fn example() -> js::Result<()> {
/// let dynamic = "data-id";
/// let base = js::obj! { id: 1, "aria-label": "save", [dynamic]: 99 }?;
/// let copy = js::obj! { ..base, id: 2 }?;
/// assert_eq!(js::get!(copy->id)?, 2);
/// assert_eq!(js::get!(copy->["aria-label"])?, "save");
/// assert_eq!(js::get!(copy->[dynamic])?, 99);
/// # Ok(())
/// # }
/// ```
#[macro_export]
macro_rules! obj {
	($($fields:tt)*) => {
		$crate::__js_obj_scan!([] $($fields)*)
	};
}

// Scans `obj!` fields in `[$($done)*] $($rest)*` to decide at compile time between an
// infallible `Value` and a fallible `Result<JsValue, JsValue>`.
//
// As long as every entry is `$key:tt : $value:expr`, entries accumulate in `[$done]`;
// if the end is reached without seeing `..` or `[key]:`, all fields are defined
// directly and `object` is returned bare. As soon as `..` or `[key]:` appears,
// passes the already-scanned `$done` entries followed by the remaining tokens to
// `__js_obj_try!` so entries still evaluate in source order and return `Result`.
#[doc(hidden)]
#[macro_export]
macro_rules! __js_obj_scan {
	([$($key:tt : $value:expr,)*]) => {{
		let object = $crate::r#macro::empty_object();
		$($crate::r#macro::define_value(
			&object,
			&$crate::__js_key!($key),
			&$crate::r#macro::arg(&$value),
		);)*
		object
	}};
	([$($done:tt)*] .. $($rest:tt)*) => {{
		let object = $crate::r#macro::empty_object();
		$crate::__js_obj_try!(object; $($done)* .. $($rest)*)
	}};
	([$($done:tt)*] [$($key:tt)*] : $($rest:tt)*) => {{
		let object = $crate::r#macro::empty_object();
		$crate::__js_obj_try!(object; $($done)* [$($key)*] : $($rest)*)
	}};
	([$($done:tt)*] $key:tt : $value:expr $(, $($rest:tt)*)?) => {
		$crate::__js_obj_scan!([$($done)* $key : $value,] $($($rest)*)?)
	};
}

// Fallible step-by-step builder for `obj!` once `__js_obj_scan!` has seen a `..source`
// spread or a `[computed]` key.
//
// Evaluates entries left-to-right into `Result<JsValue, JsValue>`, converting each
// `[key]` via `property_key` before evaluating `$value` and short-circuiting on the
// first `Err` without using `?` (which would leak out of the caller's function).
#[doc(hidden)]
#[macro_export]
macro_rules! __js_obj_try {
	($object:ident;) => {
		<$crate::Result>::Ok($object)
	};
	($object:ident; .. $source:expr $(, $($rest:tt)*)?) => {
		match $crate::r#macro::spread_into(&$object, &$crate::r#macro::arg(&$source)) {
			::core::result::Result::Ok(()) => {
				$crate::__js_obj_try!($object; $($($rest)*)?)
			}
			::core::result::Result::Err(error) => ::core::result::Result::Err(error),
		}
	};
	($object:ident; [ $key:expr ] : $value:expr $(, $($rest:tt)*)?) => {
		match $crate::r#macro::property_key(&$crate::r#macro::arg(&$key)) {
			::core::result::Result::Ok(key) => {
				let value = $crate::r#macro::arg(&$value);
				$crate::r#macro::define_value(&$object, &key, &value);
				$crate::__js_obj_try!($object; $($($rest)*)?)
			}
			::core::result::Result::Err(error) => ::core::result::Result::Err(error),
		}
	};
	($object:ident; $key:tt : $value:expr $(, $($rest:tt)*)?) => {{
		$crate::r#macro::define_value(
			&$object,
			&$crate::__js_key!($key),
			&$crate::r#macro::arg(&$value),
		);
		$crate::__js_obj_try!($object; $($($rest)*)?)
	}};
}

/// Builds a JavaScript `Array` from a list of expressions or an `IntoIterator` spread.
///
/// - `arr![]` creates an empty array.
/// - `arr![a, b, c]` converts each expression into a `Value`.
/// - `arr![..iter]` collects any `IntoIterator` whose items implement `Into<Value>`.
///
/// # Examples
///
/// ```no_run
/// # fn example() -> js::Result<()> {
/// let empty = js::arr![];
/// assert_eq!(js::get!(empty->length)?, 0);
///
/// let items = js::arr![1, "two", true];
/// assert_eq!(js::get!(items->[1])?, "two");
///
/// let from_iter = js::arr![..[10, 20, 30]];
/// assert_eq!(js::get!(from_iter->length)?, 3);
/// # Ok(())
/// # }
/// ```
#[macro_export]
macro_rules! arr {
	() => {
		$crate::r#macro::array(::core::iter::empty::<$crate::wasm_bindgen::JsValue>())
	};
	(.. $values:expr) => {
		$crate::r#macro::array($values)
	};
	($($value:expr),+ $(,)?) => {
		$crate::r#macro::array([$($crate::r#macro::arg(&$value)),+])
	};
}

/// Builds a `Value`, supporting nested `{ ... }` object and `[ ... ]` array literals,
/// `null`, `undefined`, and `..spread` in a single expression.
///
/// Like `obj!`, object keys may be identifiers (`r#type` becomes `"type"`), literals,
/// or `[computed]` expressions, `..source` spreads may appear at any position any
/// number of times (with later entries overriding earlier ones), and `"__proto__"` is
/// always defined as an own data property rather than invoking the prototype setter. As
/// in JS, a `[computed]` key is converted to a property key before its value expression
/// is evaluated.
///
/// If any object literal inside `val!`—at any nesting depth—contains a `..source`
/// spread or a `[computed]` key, `val!` returns `js::Result<Value>`, chosen at compile
/// time from the syntax alone. Otherwise it returns a plain `Value`. Note that `..iter`
/// inside an array literal iterates a Rust `IntoIterator` without running JS, so array
/// spreads do not make `val!` fallible.
///
/// # Errors
///
/// When returning `js::Result<Value>`, returns `Err` with the thrown JS value if
/// spreading an object (`..source`) or converting a `[computed]` key throws in page
/// code (such as a getter, `Proxy` trap, `toString`, `valueOf`, or
/// `[Symbol.toPrimitive]`), avoiding unwinding a JS exception through Rust stack
/// frames.
///
/// # Examples
///
/// ```no_run
/// # fn example() -> js::Result<()> {
/// let extra = ["b", "c"];
/// let payload = js::val!({
///     id: 1,
///     tags: ["a", ..extra],
///     meta: { parent: null, note: undefined },
/// });
/// assert_eq!(js::get!(payload->id)?, 1);
/// assert_eq!(js::get!(payload->tags[2])?, "c");
/// assert!(js::get!(payload->meta.parent)?.is_null());
///
/// let key = "dynamic";
/// let with_computed = js::val!({ nested: [{ [key]: 42 }] })?;
/// assert_eq!(js::get!(with_computed->nested[0].dynamic)?, 42);
/// # Ok(())
/// # }
/// ```
#[macro_export]
macro_rules! val {
	// `null`, `undefined`, `{ ... }`, and `[ ... ]` must precede `$value:expr` so they
	// are not parsed as Rust expressions (where `null`/`undefined` would be unresolved
	// identifiers and `{ ... }` / `[ ... ]` would be a Rust block or array).
	(null) => {
		$crate::wasm_bindgen::JsValue::NULL
	};
	(undefined) => {
		$crate::wasm_bindgen::JsValue::UNDEFINED
	};
	({ $($fields:tt)* }) => {
		$crate::__js_val_scan!({ $($fields)* } [(f { $($fields)* })])
	};
	([ $($items:tt)* ]) => {
		$crate::__js_val_scan!([ $($items)* ] [(i { $($items)* })])
	};
	($value:expr) => {
		$crate::r#macro::arg(&$value)
	};
}

// Compile-time scanner that checks whether a `val!` literal contains any object
// `..source` spread or `[computed]` key at any nesting depth.
//
// Keeps the original `$value:tt` intact alongside a work list `[$($work)*]` of pending
// `(f { ... })` object-field blocks and `(i { ... })` array-item blocks:
// - Empty `(f {})` and `(i {})` entries are popped from the front of the work list.
// - Seeing `..` or `[key]:` inside an `(f { ... })` entry immediately dispatches the
//   whole `$value` to `__js_val_try!` (returning `Result<JsValue, JsValue>`).
// - Nested `{ ... }` or `[ ... ]` values inside either an `(f { ... })` or
//   `(i { ... })` entry push a new `(f { ... })` or `(i { ... })` work item onto the
//   list while advancing the current block to its remaining tokens.
// - Plain field values and array items (including `.. $iter` in `(i { ... })`, which is
//   a Rust iterator) are skipped.
// - When the work list becomes empty `[]`, no fallible object operation was found, so
//   `$value` is dispatched to `__js_val_plain!` (returning a bare `JsValue`).
#[doc(hidden)]
#[macro_export]
macro_rules! __js_val_scan {
	($value:tt []) => {
		$crate::__js_val_plain!($value)
	};
	($value:tt [(f {}) $($work:tt)*]) => {
		$crate::__js_val_scan!($value [$($work)*])
	};
	($value:tt [(i {}) $($work:tt)*]) => {
		$crate::__js_val_scan!($value [$($work)*])
	};
	($value:tt [(f { .. $($rest:tt)* }) $($work:tt)*]) => {
		$crate::__js_val_try!($value)
	};
	($value:tt [(f { [$($key:tt)*] : $($rest:tt)* }) $($work:tt)*]) => {
		$crate::__js_val_try!($value)
	};
	(
		$value:tt
		[(f { $key:tt : { $($inner:tt)* } $(, $($rest:tt)*)? }) $($work:tt)*]
	) => {
		$crate::__js_val_scan!(
			$value [(f { $($($rest)*)? }) (f { $($inner)* }) $($work)*]
		)
	};
	(
		$value:tt
		[(f { $key:tt : [ $($inner:tt)* ] $(, $($rest:tt)*)? }) $($work:tt)*]
	) => {
		$crate::__js_val_scan!(
			$value [(f { $($($rest)*)? }) (i { $($inner)* }) $($work)*]
		)
	};
	($value:tt [(f { $key:tt : $field:expr $(, $($rest:tt)*)? }) $($work:tt)*]) => {
		$crate::__js_val_scan!($value [(f { $($($rest)*)? }) $($work)*])
	};
	($value:tt [(i { .. $iter:expr $(, $($rest:tt)*)? }) $($work:tt)*]) => {
		$crate::__js_val_scan!($value [(i { $($($rest)*)? }) $($work)*])
	};
	($value:tt [(i { { $($inner:tt)* } $(, $($rest:tt)*)? }) $($work:tt)*]) => {
		$crate::__js_val_scan!(
			$value [(i { $($($rest)*)? }) (f { $($inner)* }) $($work)*]
		)
	};
	($value:tt [(i { [ $($inner:tt)* ] $(, $($rest:tt)*)? }) $($work:tt)*]) => {
		$crate::__js_val_scan!(
			$value [(i { $($($rest)*)? }) (i { $($inner)* }) $($work)*]
		)
	};
	($value:tt [(i { $item:expr $(, $($rest:tt)*)? }) $($work:tt)*]) => {
		$crate::__js_val_scan!($value [(i { $($($rest)*)? }) $($work)*])
	};
}

// Infallible evaluator for `val!` literals once `__js_val_scan!` has verified that no
// nested object uses `..source` or `[computed]` keys. Returns a plain `JsValue`.
#[doc(hidden)]
#[macro_export]
macro_rules! __js_val_plain {
	(null) => {
		$crate::wasm_bindgen::JsValue::NULL
	};
	(undefined) => {
		$crate::wasm_bindgen::JsValue::UNDEFINED
	};
	({ $($fields:tt)* }) => {{
		let object = $crate::r#macro::empty_object();
		$crate::__js_val_fields!(object; $($fields)*);
		object
	}};
	([ $($items:tt)* ]) => {{
		let mut items = ::std::vec::Vec::<$crate::wasm_bindgen::JsValue>::new();
		$crate::__js_val_items!(items; $($items)*);
		$crate::r#macro::array(items)
	}};
}

// Fallible evaluator for `val!` literals when `__js_val_scan!` finds an object spread
// or computed key anywhere inside the tree. Evaluates into `Result<JsValue, JsValue>`.
#[doc(hidden)]
#[macro_export]
macro_rules! __js_val_try {
	(null) => {
		<$crate::Result>::Ok($crate::wasm_bindgen::JsValue::NULL)
	};
	(undefined) => {
		<$crate::Result>::Ok($crate::wasm_bindgen::JsValue::UNDEFINED)
	};
	({ $($fields:tt)* }) => {{
		let object = $crate::r#macro::empty_object();
		$crate::__js_val_try_fields!(object; $($fields)*)
	}};
	([ $($items:tt)* ]) => {{
		let mut items = ::std::vec::Vec::<$crate::wasm_bindgen::JsValue>::new();
		$crate::__js_val_try_items!(items; $($items)*)
	}};
	($value:expr) => {
		<$crate::Result>::Ok($crate::r#macro::arg(&$value))
	};
}

// Fallible token muncher for object fields inside `__js_val_try!`.
//
// Evaluates `..source` spreads via `spread_into` and converts `[computed]` keys via
// `property_key` before handing the resolved key and remaining tokens to
// `__js_val_try_value!` so key conversion always precedes value evaluation.
#[doc(hidden)]
#[macro_export]
macro_rules! __js_val_try_fields {
	($object:ident;) => {
		<$crate::Result>::Ok($object)
	};
	($object:ident; .. $source:expr $(, $($rest:tt)*)?) => {
		match $crate::r#macro::spread_into(&$object, &$crate::r#macro::arg(&$source)) {
			::core::result::Result::Ok(()) => {
				$crate::__js_val_try_fields!($object; $($($rest)*)?)
			}
			::core::result::Result::Err(error) => ::core::result::Result::Err(error),
		}
	};
	($object:ident; [ $key:expr ] : $($rest:tt)*) => {
		match $crate::r#macro::property_key(&$crate::r#macro::arg(&$key)) {
			::core::result::Result::Ok(key) => {
				$crate::__js_val_try_value!($object; key; $($rest)*)
			}
			::core::result::Result::Err(error) => ::core::result::Result::Err(error),
		}
	};
	($object:ident; $key:tt : $($rest:tt)*) => {{
		let key = $crate::__js_key!($key);
		$crate::__js_val_try_value!($object; key; $($rest)*)
	}};
}

// Matches the value following `$key :` in a fallible `val!` object literal.
//
// Special value forms (`null`, `undefined`, `{ ... }`, `[ ... ]`) only match when
// followed by `,` or the end of the braces and delegate to `__js_val_try_define!`;
// general Rust expressions are evaluated directly and defined on `$object`.
#[doc(hidden)]
#[macro_export]
macro_rules! __js_val_try_value {
	($object:ident; $key:ident; null $(, $($rest:tt)*)?) => {
		$crate::__js_val_try_define!($object; $key; (null); $($($rest)*)?)
	};
	($object:ident; $key:ident; undefined $(, $($rest:tt)*)?) => {
		$crate::__js_val_try_define!($object; $key; (undefined); $($($rest)*)?)
	};
	($object:ident; $key:ident; { $($value:tt)* } $(, $($rest:tt)*)?) => {
		$crate::__js_val_try_define!($object; $key; ({ $($value)* }); $($($rest)*)?)
	};
	($object:ident; $key:ident; [ $($value:tt)* ] $(, $($rest:tt)*)?) => {
		$crate::__js_val_try_define!($object; $key; ([ $($value)* ]); $($($rest)*)?)
	};
	($object:ident; $key:ident; $value:expr $(, $($rest:tt)*)?) => {{
		$crate::r#macro::define_value(&$object, &$key, &$crate::r#macro::arg(&$value));
		$crate::__js_val_try_fields!($object; $($($rest)*)?)
	}};
}

// Evaluates a nested `val!` value form via `__js_val_try!`, defines it on `$object` at
// `$key` if `Ok`, and continues with `__js_val_try_fields!` (or short-circuits `Err`).
#[doc(hidden)]
#[macro_export]
macro_rules! __js_val_try_define {
	($object:ident; $key:ident; ($($value:tt)*); $($rest:tt)*) => {
		match $crate::__js_val_try!($($value)*) {
			::core::result::Result::Ok(value) => {
				$crate::r#macro::define_value(&$object, &$key, &value);
				$crate::__js_val_try_fields!($object; $($rest)*)
			}
			::core::result::Result::Err(error) => ::core::result::Result::Err(error),
		}
	};
}

// Fallible token muncher for array elements inside `__js_val_try!`.
//
// `.. $iter` extends `$items` from a Rust `IntoIterator`; special literal forms
// (`null`, `undefined`, `{ ... }`, `[ ... ]`) delegate to `__js_val_try_push!`; other
// Rust expressions are pushed directly. Finishes by wrapping the JS `Array` in `Ok`.
#[doc(hidden)]
#[macro_export]
macro_rules! __js_val_try_items {
	($items:ident;) => {
		<$crate::Result>::Ok($crate::r#macro::array($items))
	};
	($items:ident; .. $iter:expr $(, $($rest:tt)*)?) => {{
		$items.extend(::core::iter::IntoIterator::into_iter($iter).map(
			::core::convert::Into::<$crate::wasm_bindgen::JsValue>::into,
		));
		$crate::__js_val_try_items!($items; $($($rest)*)?)
	}};
	($items:ident; null $(, $($rest:tt)*)?) => {
		$crate::__js_val_try_push!($items; (null); $($($rest)*)?)
	};
	($items:ident; undefined $(, $($rest:tt)*)?) => {
		$crate::__js_val_try_push!($items; (undefined); $($($rest)*)?)
	};
	($items:ident; { $($item:tt)* } $(, $($rest:tt)*)?) => {
		$crate::__js_val_try_push!($items; ({ $($item)* }); $($($rest)*)?)
	};
	($items:ident; [ $($item:tt)* ] $(, $($rest:tt)*)?) => {
		$crate::__js_val_try_push!($items; ([ $($item)* ]); $($($rest)*)?)
	};
	($items:ident; $item:expr $(, $($rest:tt)*)?) => {{
		$items.push($crate::r#macro::arg(&$item));
		$crate::__js_val_try_items!($items; $($($rest)*)?)
	}};
}

// Evaluates a nested `val!` array element via `__js_val_try!`, pushes it onto `$items`
// if `Ok`, and continues with `__js_val_try_items!` (or short-circuits `Err`).
#[doc(hidden)]
#[macro_export]
macro_rules! __js_val_try_push {
	($items:ident; ($($item:tt)*); $($rest:tt)*) => {
		match $crate::__js_val_try!($($item)*) {
			::core::result::Result::Ok(item) => {
				$items.push(item);
				$crate::__js_val_try_items!($items; $($rest)*)
			}
			::core::result::Result::Err(error) => ::core::result::Result::Err(error),
		}
	};
}

// Infallible token muncher for object fields inside `__js_val_plain!({ ... })` with
// state `($object:ident; $($rest)*)`.
//
// Special value forms (`null`, `undefined`, `{ ... }`, `[ ... ]`) only match when
// immediately followed by `,` or the end of the braces, so Rust expressions that start
// with braces or brackets (such as `{ 1 } + 1` or `[a, b][0]`) fall through to the
// `$value:expr` arm.
#[doc(hidden)]
#[macro_export]
macro_rules! __js_val_fields {
	($object:ident;) => {};
	($object:ident; $key:tt : null $(, $($rest:tt)*)?) => {
		$crate::__js_val_field!($object, $key, $crate::__js_val_plain!(null));
		$crate::__js_val_fields!($object; $($($rest)*)?);
	};
	($object:ident; $key:tt : undefined $(, $($rest:tt)*)?) => {
		$crate::__js_val_field!($object, $key, $crate::__js_val_plain!(undefined));
		$crate::__js_val_fields!($object; $($($rest)*)?);
	};
	($object:ident; $key:tt : { $($value:tt)* } $(, $($rest:tt)*)?) => {
		$crate::__js_val_field!($object, $key, $crate::__js_val_plain!({ $($value)* }));
		$crate::__js_val_fields!($object; $($($rest)*)?);
	};
	($object:ident; $key:tt : [ $($value:tt)* ] $(, $($rest:tt)*)?) => {
		$crate::__js_val_field!($object, $key, $crate::__js_val_plain!([ $($value)* ]));
		$crate::__js_val_fields!($object; $($($rest)*)?);
	};
	($object:ident; $key:tt : $value:expr $(, $($rest:tt)*)?) => {
		$crate::__js_val_field!($object, $key, $crate::r#macro::arg(&$value));
		$crate::__js_val_fields!($object; $($($rest)*)?);
	};
}

// Defines a single own data property with an identifier or literal `$key` on `$object`
// for `__js_val_fields!`.
#[doc(hidden)]
#[macro_export]
macro_rules! __js_val_field {
	($object:ident, $key:tt, $value:expr) => {
		$crate::r#macro::define_value(&$object, &$crate::__js_key!($key), &$value)
	};
}

// Infallible token muncher for array elements inside `__js_val_plain!([ ... ])` with
// state `($items:ident; $($rest)*)`.
//
// As in `__js_val_fields!`, literal forms (`null`, `undefined`, `{ ... }`, `[ ... ]`)
// require a trailing comma or end-of-list so compound Rust expressions such as
// `[pair[1]][0]` fall through to `$item:expr`.
#[doc(hidden)]
#[macro_export]
macro_rules! __js_val_items {
	($items:ident;) => {};
	($items:ident; .. $iter:expr $(, $($rest:tt)*)?) => {
		$items.extend(::core::iter::IntoIterator::into_iter($iter).map(
			::core::convert::Into::<$crate::wasm_bindgen::JsValue>::into,
		));
		$crate::__js_val_items!($items; $($($rest)*)?);
	};
	($items:ident; null $(, $($rest:tt)*)?) => {
		$items.push($crate::__js_val_plain!(null));
		$crate::__js_val_items!($items; $($($rest)*)?);
	};
	($items:ident; undefined $(, $($rest:tt)*)?) => {
		$items.push($crate::__js_val_plain!(undefined));
		$crate::__js_val_items!($items; $($($rest)*)?);
	};
	($items:ident; { $($item:tt)* } $(, $($rest:tt)*)?) => {
		$items.push($crate::__js_val_plain!({ $($item)* }));
		$crate::__js_val_items!($items; $($($rest)*)?);
	};
	($items:ident; [ $($item:tt)* ] $(, $($rest:tt)*)?) => {
		$items.push($crate::__js_val_plain!([ $($item)* ]));
		$crate::__js_val_items!($items; $($($rest)*)?);
	};
	($items:ident; $item:expr $(, $($rest:tt)*)?) => {
		$items.push($crate::r#macro::arg(&$item));
		$crate::__js_val_items!($items; $($($rest)*)?);
	};
}

/// Constructs a JavaScript `Error` with a `format!`-style message.
///
/// # Examples
///
/// ```no_run
/// use js::{ValueExt as _};
///
/// let field = "port";
/// let err = js::error!("invalid {field}");
/// assert!(js::is!(err, Error));
/// assert_eq!(err.message(), "invalid port");
/// ```
#[macro_export]
macro_rules! error {
	($($format:tt)+) => {
		$crate::r#macro::error(&::std::format!($($format)+))
	};
}

/// Creates a fresh empty JavaScript `{}` object.
pub fn empty_object() -> JsValue {
	Object::new().into()
}

/// Converts `key` into a JavaScript property key (`ToPropertyKey`) for `[computed]`
/// keys in `obj!` and `val!`.
///
/// Strings, symbols, and numbers are cloned directly; other values are coerced in JS
/// via `Reflect.ownKeys({ [key]: undefined })[0]`, returning `Err` if `toString`,
/// `valueOf`, or `[Symbol.toPrimitive]` throws.
pub fn property_key(key: &JsValue) -> Result<JsValue, JsValue> {
	if key.is_string() || key.is_symbol() || key.as_f64().is_some() {
		return Ok(key.clone());
	}
	property_key_raw(key)
}

/// Collects an iterator of `Into<JsValue>` items into a JS `Array`.
pub fn array<T: Into<JsValue>>(values: impl IntoIterator<Item = T>) -> JsValue {
	values.into_iter().map(Into::into).collect::<Array>().into()
}

/// Creates a JavaScript `Error` instance with `message`.
pub fn error(message: &str) -> JsValue {
	js_sys::Error::new(message).into()
}

/// Wraps a Rust closure into a callable JavaScript function `Value`.
///
/// All parameters are bound as `Value`s from the JS arguments list (missing arguments
/// become `undefined`), and a trailing `..rest` binds the remaining arguments as an
/// `Args`. Captures are always moved into the closure (`move` is optional).
///
/// Supported modifiers and forms:
/// - `func!(|a, b| body)`: shared `Fn` closure.
/// - `func!(|this, a| body)`: when the first parameter is literally named `this` (alone
///   or combined with `mut`, `once`, or `async`), receives the JS `this` receiver as
///   its first `Value` argument.
/// - `func!(mut |a| body)`: stateful `FnMut` closure.
/// - `func!(once |a| body)`: single-use `FnOnce` closure (throws in JS if called a
///   second time).
/// - `func!(async |a| async move { ... })`: asynchronous closure whose body returns a
///   `Future<Output = Result>`, exposed to JS as a function returning a `Promise`.
/// - `func!(|a| -> js::Result { ... })`: explicit return type; returning `Err(e)`
///   throws `e` as a JS exception.
///
/// Note that the returned JS function owns its Rust closure (`into_js_value`), which
/// the JS garbage collector frees once nothing holds the function. For `once` closures,
/// the Rust captures are dropped as soon as the function is called once.
///
/// # Examples
///
/// ```no_run
/// # fn example() -> js::Result<()> {
/// let add_one = js::func!(|x| x.as_f64().unwrap_or(0.0) + 1.0);
/// assert_eq!(js::call!(add_one->(41))?, 42);
///
/// let item = js::obj! { id: 7, take: js::func!(once |this| js::get!(this->id)) };
/// assert_eq!(js::call!(item->take())?, 7);
///
/// let checked = js::func!(|x| -> js::Result {
///     if x.is_undefined() { Err(js::error!("missing")) } else { Ok(x) }
/// });
/// assert!(js::call!(checked->()).is_err());
/// # Ok(())
/// # }
/// ```
#[macro_export]
macro_rules! func {
	// Leading keywords (`move`, `mut`, `once`, `async`) must be matched before the
	// plain `||` and `|` closure arms.
	(move $($t:tt)+) => { $crate::func!($($t)+) };
	(mut $($t:tt)+) => { $crate::__js_kind!(@mut $($t)+) };
	(once $($t:tt)+) => { $crate::__js_kind!(@once $($t)+) };
	(async || $($body:tt)+) => { $crate::__js_func!(@async [] $($body)+) };
	(async | $($t:tt)+) => { $crate::__js_params!(@async [] $($t)+) };
	(|| $($body:tt)+) => { $crate::__js_func!(@sync [] $($body)+) };
	(| $($t:tt)+) => { $crate::__js_params!(@sync [] $($t)+) };
}

// Strips an optional `move` after `mut` or `once` (`func!(mut move |x| ...)`), then
// dispatches on empty (`||`) vs non-empty (`|...`) parameter lists.
#[doc(hidden)]
#[macro_export]
macro_rules! __js_kind {
	(@$kind:ident move $($t:tt)+) => { $crate::__js_kind!(@$kind $($t)+) };
	(@$kind:ident || $($body:tt)+) => { $crate::__js_func!(@$kind [] $($body)+) };
	(@$kind:ident | $($t:tt)+) => { $crate::__js_params!(@$kind [] $($t)+) };
}

// Token muncher with state `@$kind [$($p)*] $($rest)*` that collects closure parameter
// tokens up to the closing `|`.
#[doc(hidden)]
#[macro_export]
macro_rules! __js_params {
	(@$kind:ident [$($p:tt)*] | $($body:tt)+) => {
		$crate::__js_func!(@$kind [$($p)*] $($body)+)
	};
	(@$kind:ident [$($p:tt)*] $next:tt $($rest:tt)*) => {
		$crate::__js_params!(@$kind [$($p)* $next] $($rest)*)
	};
}

// Checks whether the parameter list starts with an identifier so `__js_this!` can test
// if it is `this`. Patterns like `..rest` are not a single `ident` and go straight to
// `__js_make!`.
#[doc(hidden)]
#[macro_export]
macro_rules! __js_func {
	(@$kind:ident [$first:ident $($p:tt)*] $($body:tt)+) => {
		$crate::__js_this!(@$kind $first; $first [$($p)*] $($body)+)
	};
	(@$kind:ident [$($p:tt)*] $($body:tt)+) => {
		$crate::__js_make!(@$kind [] [$($p)*] $($body)+)
	};
}

// Detects a leading `this` parameter by matching the literal token `this` in the first
// position while keeping `$this:ident` with the caller's hygiene span for binding in
// the closure body.
#[doc(hidden)]
#[macro_export]
macro_rules! __js_this {
	(@$kind:ident this; $this:ident [$(, $($p:tt)*)?] $($body:tt)+) => {
		$crate::__js_make!(@$kind [$this] [$($($p)*)?] $($body)+)
	};
	(@$kind:ident $other:ident; $first:ident [$($p:tt)*] $($body:tt)+) => {
		$crate::__js_make!(@$kind [] [$first $($p)*] $($body)+)
	};
}

// Builds the underlying Rust closure and passes it to the constructor selected by
// `__js_maker!`.
// The `-> $ret:ty $block:block` arm must precede `$body:expr` so an explicit return
// type annotation is not rejected as an expression.
#[doc(hidden)]
#[macro_export]
macro_rules! __js_make {
	(@build $kind:ident [$this:ident] [$($p:tt)*] [$ret:ty] $block:block) => {
		$crate::__js_maker!(@$kind this)(
			move |$this: $crate::Value, args: $crate::Args| -> $ret {
				$crate::__js_bind!(args; 0u32; $($p)*);
				$block
			},
		)
	};
	(@build $kind:ident [] [$($p:tt)*] [$ret:ty] $block:block) => {
		$crate::__js_maker!(@$kind)(move |args: $crate::Args| -> $ret {
			$crate::__js_bind!(args; 0u32; $($p)*);
			$block
		})
	};
	(@$kind:ident [$($this:ident)?] [$($p:tt)*] -> $ret:ty $block:block) => {
		$crate::__js_make!(@build $kind [$($this)?] [$($p)*] [$ret] $block)
	};
	(@$kind:ident [$($this:ident)?] [$($p:tt)*] $body:expr) => {
		$crate::__js_make!(@build $kind [$($this)?] [$($p)*] [_] { $body })
	};
}

// Selects the runtime wrapper function in `$crate::r#macro` for `@$kind` and optional
// `this`.
#[doc(hidden)]
#[macro_export]
macro_rules! __js_maker {
	(@sync this) => {
		$crate::r#macro::this_function
	};
	(@sync) => {
		$crate::r#macro::function
	};
	(@mut this) => {
		$crate::r#macro::this_function_mut
	};
	(@mut) => {
		$crate::r#macro::function_mut
	};
	(@once this) => {
		$crate::r#macro::this_function_once
	};
	(@once) => {
		$crate::r#macro::function_once
	};
	(@async this) => {
		$crate::r#macro::this_async_function
	};
	(@async) => {
		$crate::r#macro::async_function
	};
}

// Binds positional parameters and an optional trailing `..rest` from `$args: Args`.
// `.. $rest:ident` must precede `$name:tt` so rest parameters are not munched as a
// positional parameter.
#[doc(hidden)]
#[macro_export]
macro_rules! __js_bind {
	($args:ident; $index:expr;) => {
		let _ = &$args;
	};
	($args:ident; $index:expr; .. $rest:ident) => {
		let $rest = $args.skip($index);
	};
	($args:ident; $index:expr; $name:tt $(, $($p:tt)*)?) => {
		let $name = $args.get($index);
		$crate::__js_bind!($args; $index + 1; $($($p)*)?);
	};
}

/// Wraps a shared `Fn(Args) -> R` closure as a variadic JavaScript function.
pub fn function<R: ReturnWasmAbi + 'static>(
	f: impl Fn(Args) -> R + 'static,
) -> JsValue {
	let closure = Closure::<dyn Fn(Array) -> R>::new(move |args| f(Args(args)));
	variadic_raw(closure.into_js_value())
}

/// Wraps a shared `Fn(JsValue, Args) -> R` closure as a JavaScript method that forwards
/// its `this` receiver and arguments.
pub fn this_function<R: ReturnWasmAbi + 'static>(
	f: impl Fn(JsValue, Args) -> R + 'static,
) -> JsValue {
	let closure = Closure::<dyn Fn(JsValue, Array) -> R>::new(move |this, args| {
		f(this, Args(args))
	});
	method_raw(closure.into_js_value())
}

/// Wraps a mutable `FnMut(Args) -> R` closure as a variadic JavaScript function.
pub fn function_mut<R: ReturnWasmAbi + 'static>(
	mut f: impl FnMut(Args) -> R + 'static,
) -> JsValue {
	let closure = Closure::<dyn FnMut(Array) -> R>::new(move |args| f(Args(args)));
	variadic_raw(closure.into_js_value())
}

/// Wraps a mutable `FnMut(JsValue, Args) -> R` closure as a JavaScript method that
/// forwards its `this` receiver and arguments.
pub fn this_function_mut<R: ReturnWasmAbi + 'static>(
	mut f: impl FnMut(JsValue, Args) -> R + 'static,
) -> JsValue {
	let closure = Closure::<dyn FnMut(JsValue, Array) -> R>::new(move |this, args| {
		f(this, Args(args))
	});
	method_raw(closure.into_js_value())
}

/// Wraps a single-use `FnOnce(Args) -> R` closure as a variadic JavaScript function.
pub fn function_once<R: ReturnWasmAbi + 'static>(
	f: impl FnOnce(Args) -> R + 'static,
) -> JsValue {
	variadic_raw(Closure::once_into_js(move |args: Array| f(Args(args))))
}

/// Wraps a single-use `FnOnce(JsValue, Args) -> R` closure as a JavaScript method that
/// forwards its `this` receiver and arguments.
pub fn this_function_once<R: ReturnWasmAbi + 'static>(
	f: impl FnOnce(JsValue, Args) -> R + 'static,
) -> JsValue {
	method_raw(Closure::once_into_js(move |this: JsValue, args: Array| {
		f(this, Args(args))
	}))
}

/// Wraps an async closure returning `Future<Output = JsResult>` as a JavaScript
/// function that returns a `Promise`.
pub fn async_function<F>(f: impl Fn(Args) -> F + 'static) -> JsValue
where
	F: Future<Output = JsResult> + 'static,
{
	function(move |args| promise(f(args)))
}

/// Wraps an async closure returning `Future<Output = JsResult>` as a JavaScript method
/// that forwards its `this` receiver and arguments and returns a `Promise`.
pub fn this_async_function<F>(f: impl Fn(JsValue, Args) -> F + 'static) -> JsValue
where
	F: Future<Output = JsResult> + 'static,
{
	this_function(move |this, args| promise(f(this, args)))
}

/// Creates a JavaScript `Proxy` wrapping `$target` with the specified handler traps.
///
/// Supported trap names match the methods on `ProxyBuilder`: `get`, `set`, `has`,
/// `define_property`, and `delete_property`.
///
/// # Errors
///
/// Returns `Err` with a `TypeError` if `$target` is not an object.
///
/// # Examples
///
/// ```no_run
/// use js::Value;
///
/// # fn example() -> js::Result<()> {
/// let target = js::obj! { secret: 42 };
/// let proxy = js::proxy!(target, {
///     get: |target: &Value, key: &Value| {
///         if key.as_string().as_deref() == Some("secret") {
///             return Ok(Value::from(0));
///         }
///         js::get!(target->[key])
///     },
/// })?;
/// assert_eq!(js::get!(proxy->secret)?, 0);
/// assert!(js::proxy!(Value::NULL, {}).is_err());
/// # Ok(())
/// # }
/// ```
#[macro_export]
macro_rules! proxy {
	($target:expr, { $($trap:ident : $f:expr),* $(,)? }) => {
		$crate::r#macro::ProxyBuilder::default()
			$(.$trap($f))*
			.build(&$crate::r#macro::arg(&$target))
	};
}

/// Builder used by `proxy!` to assemble a JavaScript `Proxy` handler object.
#[derive(Default)]
pub struct ProxyBuilder {
	traps: Vec<(&'static str, JsValue)>,
}

impl ProxyBuilder {
	fn trap(mut self, name: &'static str, trap: JsValue) -> Self {
		self.traps.push((name, trap));
		self
	}

	/// Registers a `get(target, key)` trap.
	pub fn get(self, f: impl Fn(&JsValue, &JsValue) -> JsResult + 'static) -> Self {
		self.trap("get", crate::func!(|target, key| -> JsResult { f(&target, &key) }))
	}

	/// Registers a `set(target, key, value)` trap returning whether the write succeeded.
	pub fn set(
		self,
		f: impl Fn(&JsValue, &JsValue, &JsValue) -> JsResult<bool> + 'static,
	) -> Self {
		self.trap(
			"set",
			crate::func!(|target, key, value| -> JsResult<bool> {
				f(&target, &key, &value)
			}),
		)
	}

	/// Registers a `has(target, key)` trap for the `in` operator.
	pub fn has(
		self,
		f: impl Fn(&JsValue, &JsValue) -> JsResult<bool> + 'static,
	) -> Self {
		self.trap(
			"has",
			crate::func!(|target, key| -> JsResult<bool> { f(&target, &key) }),
		)
	}

	/// Registers a `defineProperty(target, key, descriptor)` trap.
	pub fn define_property(
		self,
		f: impl Fn(&JsValue, &JsValue, &JsValue) -> JsResult<bool> + 'static,
	) -> Self {
		self.trap(
			"defineProperty",
			crate::func!(|target, key, descriptor| -> JsResult<bool> {
				f(&target, &key, &descriptor)
			}),
		)
	}

	/// Registers a `deleteProperty(target, key)` trap for the `delete` operator.
	pub fn delete_property(
		self,
		f: impl Fn(&JsValue, &JsValue) -> JsResult<bool> + 'static,
	) -> Self {
		self.trap(
			"deleteProperty",
			crate::func!(|target, key| -> JsResult<bool> { f(&target, &key) }),
		)
	}

	/// Constructs the `Proxy` around `target` with all registered traps.
	pub fn build(self, target: &JsValue) -> JsResult {
		let handler = crate::object(self.traps);
		crate::new!(globalThis->Proxy(target, handler))
	}
}

// Inline JS helpers for operations that `js_sys::Reflect` either does not expose or
// handles differently from native JS syntax (for example, `target[key]` works on
// primitives by auto-boxing whereas `Reflect.get` throws a `TypeError`).
#[wasm_bindgen(inline_js = "
export function variadic(f) { return (...args) => f(args); }
export function propertyKey(key) { return Reflect.ownKeys({ [key]: undefined })[0]; }
export function defineValue(target, key, value) {
	const descriptor = { value, writable: true, enumerable: true, configurable: true };
	Object.defineProperty(target, key, descriptor);
}
export function spreadInto(target, source) {
	const copy = { ...source };
	for (const key of Reflect.ownKeys(copy)) {
		defineValue(target, key, copy[key]);
	}
}
export function method(f) { return { m(...args) { return f(this, args); } }.m; }
export function get(target, key) { return target[key]; }
export function hasOwn(target, key) { return Object.hasOwn(target, key); }
export function instanceOf(value, constructor) { return value instanceof constructor; }
export function string(value) { return String(value); }
export function number(value) { return Number(value); }
export function arrayLike(value) { return Array.prototype.slice.call(value); }
export function spreadIterable(value) { return [...value]; }
")]
extern "C" {
	#[wasm_bindgen(js_name = variadic)]
	fn variadic_raw(f: JsValue) -> JsValue;
	#[wasm_bindgen(js_name = method)]
	fn method_raw(f: JsValue) -> JsValue;
	#[wasm_bindgen(catch, js_name = propertyKey)]
	fn property_key_raw(key: &JsValue) -> Result<JsValue, JsValue>;
	/// Defines an own writable, enumerable, configurable data property on `target` via
	/// `Object.defineProperty`.
	#[wasm_bindgen(js_name = defineValue)]
	pub fn define_value(target: &JsValue, key: &JsValue, value: &JsValue);
	/// Shallow-copies own enumerable properties from `source` onto `target` via
	/// `Object.defineProperty` so `"__proto__"` is defined as an own data property,
	/// returning `Err` if spreading `source` throws (for example in a getter or `Proxy`
	/// trap).
	#[wasm_bindgen(catch, js_name = spreadInto)]
	pub fn spread_into(target: &JsValue, source: &JsValue) -> Result<(), JsValue>;
	/// Reads `target[key]` using native JS indexing (supporting primitive receivers).
	#[wasm_bindgen(catch)]
	pub fn get(target: &JsValue, key: &JsValue) -> Result<JsValue, JsValue>;
	/// Calls `Object.hasOwn(target, key)`.
	#[wasm_bindgen(catch, js_name = hasOwn)]
	pub fn own(target: &JsValue, key: &JsValue) -> Result<bool, JsValue>;
	#[wasm_bindgen(catch, js_name = instanceOf)]
	fn instance_of_raw(value: &JsValue, constructor: &JsValue) -> Result<bool, JsValue>;
	/// Converts any JavaScript value to a `String` using `String(value)`.
	///
	/// Unlike `ValueExt::text`, this performs full JS string coercion (for example,
	/// `12` becomes `"12"`, `null` becomes `"null"`, and symbols format as
	/// `"Symbol(...)"`).
	///
	/// # Errors
	///
	/// Returns `Err` if `value` is an object whose `toString` method throws or is
	/// missing (such as `Object.create(null)`).
	///
	/// # Examples
	///
	/// ```no_run
	/// # fn example() -> js::Result<()> {
	/// assert_eq!(js::string(&js::Value::from(42))?, "42");
	/// assert_eq!(js::string(&js::Value::NULL)?, "null");
	/// assert!(js::string(&js::dict()).is_err());
	/// # Ok(())
	/// # }
	/// ```
	#[wasm_bindgen(catch)]
	pub fn string(value: &JsValue) -> Result<String, JsValue>;
	#[wasm_bindgen(catch)]
	pub(crate) fn number(value: &JsValue) -> Result<f64, JsValue>;
	#[wasm_bindgen(catch, js_name = arrayLike)]
	pub(crate) fn array_like(value: &JsValue) -> Result<Array, JsValue>;
	#[wasm_bindgen(catch, js_name = spreadIterable)]
	pub(crate) fn spread_iterable(value: &JsValue) -> Result<Array, JsValue>;
}
