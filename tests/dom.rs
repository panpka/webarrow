//! Browser integration tests for DOM helpers (`create_element`, `element`,
//! `attach_shadow`, `csp_nonce`) and `Observer` (`dom` feature).

#![cfg(all(target_arch = "wasm32", feature = "dom"))]

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{Log, Probe, document, log, sleep};
use js::{
	self as js, Mutation, ObserveOptions, Observer, ValueExt as _, attach_shadow,
	create_element, csp_nonce, element,
};
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
fn create_element_returns_html_element_with_tag() {
	let div = create_element(&document(), "div").unwrap();
	assert_eq!(div.tag_name(), "DIV");
	assert!(js::is!(div, HTMLElement));
}

#[wasm_bindgen_test]
fn element_applies_inline_css_without_attaching() {
	let span = element(&document(), "span", "color: red;").unwrap();
	assert_eq!(span.tag_name(), "SPAN");
	assert_eq!(span.style().get_property_value("color").unwrap(), "red");
	assert_eq!(js::get!(span->isConnected).unwrap(), false);
	assert!(js::get!(span->parentNode).unwrap().is_null());
}

#[wasm_bindgen_test]
fn attach_shadow_opens_shadow_root_once() {
	let host = create_element(&document(), "div").unwrap();
	let root = attach_shadow(&host).unwrap();
	assert_eq!(js::get!(root->mode).unwrap(), "open");
	assert_eq!(js::get!(host->shadowRoot).unwrap(), js::Value::from(root));
	assert!(attach_shadow(&host).is_err());
}

#[wasm_bindgen_test]
fn csp_nonce_reads_nonce_of_first_script() {
	let document = document();
	let script = create_element(&document, "script").unwrap();
	script.set_attribute("nonce", "abc123").unwrap();
	let body = document.body().unwrap();
	body.append_child(&script).unwrap();
	let nonce = csp_nonce(&document);
	body.remove_child(&script).unwrap();
	assert_eq!(nonce.unwrap(), "abc123");
}

#[wasm_bindgen_test]
fn csp_nonce_rejects_missing_or_empty_nonce() {
	let document = document();
	let missing = csp_nonce(&document).unwrap_err();
	assert_eq!(missing.message(), "CSP nonce is unavailable");

	let script = create_element(&document, "script").unwrap();
	script.set_attribute("nonce", "").unwrap();
	let body = document.body().unwrap();
	body.append_child(&script).unwrap();
	let empty = csp_nonce(&document);
	body.remove_child(&script).unwrap();
	assert_eq!(empty.unwrap_err().message(), "CSP nonce is unavailable");
}

// Flattens each batch of `Mutation`s delivered to an `Observer` callback into a shared
// `Log<Mutation>`.
fn recorder() -> (Log<Mutation>, impl Fn(Vec<Mutation>) + 'static) {
	let (mutations, push) = log();
	(mutations, move |batch: Vec<Mutation>| batch.into_iter().for_each(&push))
}

fn child_list() -> ObserveOptions {
	ObserveOptions { child_list: true, ..Default::default() }
}

// Appends a `<p>` element and yields to the microtask/timer queue so `MutationObserver`
// callbacks run before assertions.
async fn append_paragraph(parent: &web_sys::Node) {
	let document = parent.owner_document().unwrap();
	parent.append_child(&create_element(&document, "p").unwrap()).unwrap();
	sleep(0).await;
}

#[wasm_bindgen_test]
async fn observer_reports_added_children() {
	let document = document();
	let host = create_element(&document, "div").unwrap();
	let (mutations, callback) = recorder();
	let _observer =
		Observer::observe(&host.clone().into(), child_list(), callback).unwrap();

	let child = create_element(&document, "p").unwrap();
	host.append_child(&child).unwrap();
	sleep(0).await;

	let mutations = mutations.borrow();
	assert_eq!(mutations.len(), 1);
	assert_eq!(mutations[0].target.clone().unwrap(), js::Value::from(host));
	assert_eq!(mutations[0].added_nodes.to_vec(), vec![js::Value::from(child)]);
}

#[wasm_bindgen_test]
async fn observer_reports_character_data_in_subtree() {
	let document = document();
	let host = create_element(&document, "div").unwrap();
	let child = create_element(&document, "p").unwrap();
	let text = js::call!(document->createTextNode("a")).unwrap();
	js::call!(child->append(text)).unwrap();
	host.append_child(&child).unwrap();

	let (mutations, callback) = recorder();
	let options =
		ObserveOptions { subtree: true, character_data: true, ..Default::default() };
	let _observer = Observer::observe(&host.into(), options, callback).unwrap();

	js::set!(text->data = "b").unwrap();
	sleep(0).await;

	let mutations = mutations.borrow();
	assert_eq!(mutations.len(), 1);
	assert_eq!(mutations[0].target.clone().unwrap(), text);
	assert!(mutations[0].added_nodes.is_empty());
}

#[wasm_bindgen_test]
async fn observer_without_subtree_ignores_descendants() {
	let document = document();
	let host = create_element(&document, "div").unwrap();
	let child = create_element(&document, "p").unwrap();
	host.append_child(&child).unwrap();

	let (mutations, callback) = recorder();
	let _observer = Observer::observe(&host.into(), child_list(), callback).unwrap();

	append_paragraph(&child).await;
	assert!(mutations.borrow().is_empty());
}

#[wasm_bindgen_test]
async fn observer_stops_after_disconnect() {
	let document = document();
	let host = create_element(&document, "div").unwrap();
	let (mutations, callback) = recorder();
	let observer =
		Observer::observe(&host.clone().into(), child_list(), callback).unwrap();

	append_paragraph(&host).await;
	assert_eq!(mutations.borrow().len(), 1);

	observer.disconnect();
	append_paragraph(&host).await;
	assert_eq!(mutations.borrow().len(), 1);
}

// Providing a non-empty `attribute_filter` enables attribute observation even when
// `attributes: false`.
#[wasm_bindgen_test]
async fn observer_reports_filtered_attribute_changes() {
	let host = create_element(&document(), "div").unwrap();
	let (mutations, callback) = recorder();
	let options = ObserveOptions { attribute_filter: &["data-a"], ..Default::default() };
	let _observer = Observer::observe(&host.clone().into(), options, callback).unwrap();

	host.set_attribute("data-a", "1").unwrap();
	host.set_attribute("data-b", "2").unwrap();
	sleep(0).await;

	let mutations = mutations.borrow();
	assert_eq!(mutations.len(), 1);
	assert_eq!(mutations[0].attribute_name.as_deref(), Some("data-a"));
	assert_eq!(mutations[0].record.type_(), "attributes");
	assert!(mutations[0].added_nodes.is_empty());
}

// `Observer` is `Rc`-backed: dropping one clone keeps watching, while dropping the
// last clone disconnects the observer and drops the closure captures.
#[wasm_bindgen_test]
async fn observer_stops_and_releases_when_last_handle_dropped() {
	let document = document();
	let host = create_element(&document, "div").unwrap();
	let (mutations, callback) = recorder();
	let probe = Probe::default();
	let held = probe.clone();
	let observer = Observer::observe(&host.clone().into(), child_list(), move |batch| {
		let _ = &held;
		callback(batch);
	})
	.unwrap();
	let copy = observer.clone();
	assert!(probe.held());

	drop(observer);
	append_paragraph(&host).await;
	assert_eq!(mutations.borrow().len(), 1);

	drop(copy);
	assert!(!probe.held());
	append_paragraph(&host).await;
	assert_eq!(mutations.borrow().len(), 1);
}

// Dropping the `Observer` from inside its own callback must not panic or hit a
// borrow conflict.
#[wasm_bindgen_test]
async fn observer_can_drop_itself_inside_callback() {
	let document = document();
	let host = create_element(&document, "div").unwrap();
	let slot: Rc<RefCell<Option<Observer>>> = Rc::default();
	let (calls, push) = log();
	let observer = {
		let slot = slot.clone();
		Observer::observe(&host.clone().into(), child_list(), move |mutations| {
			let own = slot.borrow_mut().take();
			drop(own);
			push(mutations.len());
		})
		.unwrap()
	};
	*slot.borrow_mut() = Some(observer);

	append_paragraph(&host).await;
	append_paragraph(&host).await;
	assert_eq!(*calls.borrow(), vec![1]);
}

#[wasm_bindgen_test]
fn observe_rejects_non_node_target() {
	assert!(Observer::observe(&"node".into(), child_list(), |_| {}).is_err());
	assert!(Observer::observe(&js::obj! {}, child_list(), |_| {}).is_err());
}

#[wasm_bindgen_test]
fn observe_options_default_is_all_off() {
	let options = ObserveOptions::default();
	assert!(!options.child_list);
	assert!(!options.subtree);
	assert!(!options.character_data);
}
