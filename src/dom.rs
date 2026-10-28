//! DOM element construction, Shadow DOM, CSP nonces, and `MutationObserver` wrappers
//! (`dom` feature).

use std::rc::Rc;

use crate::{self as js, List, Result, Value, ValueExt as _};
use js_sys::Array;
use wasm_bindgen::JsCast as _;
use wasm_bindgen::prelude::Closure;
pub use web_sys::{Document, Element, HtmlElement, ShadowRoot, Window};
use web_sys::{
	MutationObserver, MutationObserverInit, MutationRecord, Node, ShadowRootInit,
	ShadowRootMode,
};

/// Creates an detached `HtmlElement` with `tag` via `document.createElement(tag)`.
///
/// # Errors
///
/// Returns `Err` if `tag` is not a valid element tag name (`InvalidCharacterError`).
///
/// # Examples
///
/// ```no_run
/// # fn example(document: &js::Document) -> js::Result<()> {
/// let div = js::create_element(document, "div")?;
/// assert_eq!(div.tag_name(), "DIV");
/// assert!(js::create_element(document, "bad tag").is_err());
/// # Ok(())
/// # }
/// ```
pub fn create_element(document: &Document, tag: &str) -> Result<HtmlElement> {
	Ok(document.create_element(tag)?.unchecked_into())
}

/// Creates a detached `HtmlElement` with `tag` and applies `css` to its inline
/// `style.cssText`.
///
/// # Errors
///
/// Returns `Err` if `tag` is not a valid element tag name.
///
/// # Examples
///
/// ```no_run
/// # fn example(document: &js::Document) -> js::Result<()> {
/// let badge = js::element(document, "span", "color: red; display: inline-block;")?;
/// assert_eq!(badge.tag_name(), "SPAN");
/// assert!(js::element(document, "bad tag", "").is_err());
/// # Ok(())
/// # }
/// ```
pub fn element(document: &Document, tag: &str, css: &str) -> Result<HtmlElement> {
	let element = create_element(document, tag)?;
	element.style().set_css_text(css);
	Ok(element)
}

/// Attaches an open `ShadowRoot` (`mode: "open"`) to `host`.
///
/// # Errors
///
/// Returns `Err` if `host` does not support a shadow root or already has one attached
/// (`NotSupportedError`).
///
/// # Examples
///
/// ```no_run
/// # fn example(document: &js::Document) -> js::Result<()> {
/// let host = js::create_element(document, "div")?;
/// let shadow = js::attach_shadow(&host)?;
/// assert!(shadow.host() == *host.as_ref());
/// assert!(js::attach_shadow(&host).is_err());
/// # Ok(())
/// # }
/// ```
pub fn attach_shadow(host: &Element) -> Result<ShadowRoot> {
	host.attach_shadow(&ShadowRootInit::new(ShadowRootMode::Open))
}

/// Reads the Content Security Policy nonce from the first `<script nonce="...">`
/// element in `document`.
///
/// # Errors
///
/// Returns `Err` if no `<script nonce>` element exists or if its `nonce` property is
/// empty.
///
/// # Examples
///
/// ```no_run
/// # fn example(document: &js::Document) -> js::Result<()> {
/// match js::csp_nonce(document) {
///     Ok(nonce) => assert!(!nonce.is_empty()),
///     Err(err) => assert!(js::is!(err, Error)),
/// }
/// # Ok(())
/// # }
/// ```
pub fn csp_nonce(document: &Document) -> Result<String> {
	let script = document
		.query_selector("script[nonce]")?
		.ok_or_else(|| js::error!("CSP nonce is unavailable"))?;
	let nonce = js::get!(script->nonce)?.text();
	if nonce.is_empty() {
		return Err(js::error!("CSP nonce is unavailable"));
	}
	Ok(nonce)
}

/// A reference-counted `MutationObserver` handle that disconnects automatically when
/// its last clone is dropped.
///
/// Cloning an `Observer` shares the same underlying `MutationObserver` and callback.
///
/// # Examples
///
/// ```no_run
/// use js::{ObserveOptions, Observer};
///
/// # fn example(document: &js::Document) -> js::Result<()> {
/// let host: js::Value = js::create_element(document, "div")?.into();
/// let options = ObserveOptions { child_list: true, ..Default::default() };
/// let observer = Observer::observe(&host, options, |mutations| {
///     let _ = mutations.len();
/// })?;
/// observer.disconnect();
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug)]
pub struct Observer(Rc<Watch>);

#[derive(Debug)]
struct Watch {
	observer: MutationObserver,
	_callback: Closure<dyn FnMut(Array)>,
}

impl Drop for Watch {
	fn drop(&mut self) {
		self.observer.disconnect();
	}
}

impl Observer {
	/// Starts observing `target` (which must be a DOM `Node`) with `options`, invoking
	/// `callback` with each batch of `Mutation`s.
	///
	/// If `options.attribute_filter` is non-empty, attribute observation is enabled
	/// automatically even when `options.attributes` is `false`.
	///
	/// # Errors
	///
	/// Returns `Err` if `target` is not a `Node` or if the `MutationObserver` rejects
	/// `options` (for example when no observation types are enabled).
	///
	/// # Examples
	///
	/// ```no_run
	/// use js::{ObserveOptions, Observer};
	///
	/// # fn example(document: &js::Document) -> js::Result<()> {
	/// let host: js::Value = js::create_element(document, "div")?.into();
	/// let options = ObserveOptions { child_list: true, ..Default::default() };
	/// let _observer = Observer::observe(&host, options, |_batch| {})?;
	/// assert!(Observer::observe(&js::Value::NULL, options, |_| {}).is_err());
	/// # Ok(())
	/// # }
	/// ```
	pub fn observe(
		target: &Value,
		options: ObserveOptions,
		mut callback: impl FnMut(Vec<Mutation>) + 'static,
	) -> Result<Self> {
		let callback = Closure::<dyn FnMut(Array)>::new(move |records: Array| {
			let mutations = records
				.iter()
				.map(|record| Mutation::from_record(record.unchecked_into()))
				.collect();
			callback(mutations);
		});
		let observer = MutationObserver::new(callback.as_ref().unchecked_ref())?;
		let init = MutationObserverInit::new();
		init.set_child_list(options.child_list);
		init.set_subtree(options.subtree);
		init.set_character_data(options.character_data);
		init.set_attributes(options.attributes || !options.attribute_filter.is_empty());
		if !options.attribute_filter.is_empty() {
			let filter = options.attribute_filter.iter().map(|name| Value::from(*name));
			init.set_attribute_filter(&filter.collect::<Array>());
		}
		observer.observe_with_options(target.unchecked_ref::<Node>(), &init)?;
		Ok(Self(Rc::new(Watch { observer, _callback: callback })))
	}

	/// Stops the `MutationObserver` from delivering further mutations without waiting
	/// for all `Observer` clones to be dropped.
	pub fn disconnect(&self) {
		self.0.observer.disconnect();
	}
}

/// Configuration flags passed to `Observer::observe`.
#[derive(Clone, Copy, Debug, Default)]
pub struct ObserveOptions {
	/// Observe additions and removals of direct child nodes (`childList`).
	pub child_list: bool,
	/// Extend observation to the entire descendant subtree of the target (`subtree`).
	pub subtree: bool,
	/// Observe changes to text and character data nodes (`characterData`).
	pub character_data: bool,
	/// Observe changes to element attributes (`attributes`).
	pub attributes: bool,
	/// Restrict attribute observation to this list of attribute local names
	/// (`attributeFilter`). A non-empty slice implies `attributes: true`.
	pub attribute_filter: &'static [&'static str],
}

/// A single DOM mutation record delivered to an `Observer` callback.
#[derive(Clone, Debug)]
pub struct Mutation {
	/// The node affected by the mutation (`record.target`).
	pub target: Option<Value>,
	/// Nodes added by a `childList` mutation, copied once into a JS-`Array`-backed
	/// `List` (`List<Value>`) with `Array.from(record.addedNodes)`.
	///
	/// Because the snapshot stays in the backing JS `Array`, nodes are converted to
	/// `Value` only when read; call `.to_vec()` to collect them into a `Vec<Value>`.
	pub added_nodes: List,
	/// Name of the changed attribute for an `attributes` mutation
	/// (`record.attributeName`).
	pub attribute_name: Option<String>,
	/// The underlying `web_sys::MutationRecord`.
	pub record: MutationRecord,
}

impl Mutation {
	fn from_record(record: MutationRecord) -> Self {
		Self {
			target: record.target().map(Into::into),
			added_nodes: List::from_array(Array::from(&record.added_nodes())),
			attribute_name: record.attribute_name(),
			record,
		}
	}
}
