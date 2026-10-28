//! Browser integration tests for `Timer`, `sleep`, `timeout`, `frames`, `Listener`,
//! `next_event`, and `events` (`event` feature).

#![cfg(all(target_arch = "wasm32", feature = "event"))]

mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::{Probe, counter, document, log};
use futures_util::{FutureExt as _, StreamExt as _};
use js::ValueExt as _;
use js::{ListenOptions, Listener, Timer, events, frames, next_event, sleep, timeout};
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

wasm_bindgen_test_configure!(run_in_browser);

// Polls `done()` at 10ms intervals (up to 500ms) for animation-frame tests where exact
// repaint timing depends on the headless browser.
async fn wait_until(done: impl Fn() -> bool) {
	for _ in 0..50 {
		if done() {
			return;
		}
		sleep(10).await;
	}
}

#[wasm_bindgen_test]
async fn timeout_runs_once_when_detached() {
	let (count, tick) = counter();
	Timer::timeout(0, tick).unwrap().detach();
	assert_eq!(count.get(), 0);
	sleep(30).await;
	assert_eq!(count.get(), 1);
}

#[wasm_bindgen_test]
async fn timeout_is_cancelled_on_drop() {
	let (count, tick) = counter();
	let timer = Timer::timeout(10, tick).unwrap();
	drop(timer);
	sleep(40).await;
	assert_eq!(count.get(), 0);
}

#[wasm_bindgen_test]
fn dropping_timer_releases_callback_immediately() {
	let probe = Probe::default();
	let held = probe.clone();
	let timeout = Timer::timeout(1000, move || drop(held)).unwrap();
	assert!(probe.held());
	drop(timeout);
	assert!(!probe.held());

	let held = probe.clone();
	let interval = Timer::interval(1000, move || {
		let _ = &held;
	});
	assert!(probe.held());
	drop(interval.unwrap());
	assert!(!probe.held());

	let held = probe.clone();
	let frame = Timer::frame(move |_| drop(held)).unwrap();
	assert!(probe.held());
	drop(frame);
	assert!(!probe.held());
}

#[wasm_bindgen_test]
async fn interval_repeats_until_dropped() {
	let (count, tick) = counter();
	let timer = Timer::interval(5, tick).unwrap();
	sleep(80).await;
	let ran = count.get();
	assert!(ran >= 2, "ran {ran} times");
	drop(timer);
	sleep(40).await;
	assert_eq!(count.get(), ran);
}

#[wasm_bindgen_test]
async fn frame_passes_timestamp() {
	let time = Rc::new(Cell::new(None));
	let seen = time.clone();
	Timer::frame(move |now| seen.set(Some(now))).unwrap().detach();
	wait_until(|| time.get().is_some()).await;
	assert!(time.get().is_some_and(|now| now > 0.0));
}

#[wasm_bindgen_test]
async fn frame_is_cancelled_on_drop() {
	let (count, tick) = counter();
	drop(Timer::frame(move |_| tick()).unwrap());
	let (done, finish) = counter();
	Timer::frame(move |_| finish()).unwrap().detach();
	wait_until(|| done.get() > 0).await;
	assert_eq!(done.get(), 1);
	assert_eq!(count.get(), 0);
}

// Simulates a non-rendering environment where `requestAnimationFrame` is absent.
#[wasm_bindgen_test]
fn frame_errors_without_request_animation_frame() {
	let original = js::get!(globalThis->requestAnimationFrame).unwrap();
	js::set!(globalThis->requestAnimationFrame = js::Value::UNDEFINED).unwrap();
	let result = Timer::frame(|_| {});
	js::set!(globalThis->requestAnimationFrame = original).unwrap();
	assert!(result.is_err());
}

#[wasm_bindgen_test]
async fn sleep_waits_at_least_requested_ms() {
	let now = || js::call!(globalThis->performance.now()).unwrap().as_f64().unwrap();
	let start = now();
	sleep(50).await;
	assert!(now() - start >= 40.0);
}

#[wasm_bindgen_test]
async fn timeout_returns_none_after_deadline() {
	assert_eq!(timeout(async { 1 }, 50).await, Some(1));
	let now = || js::call!(globalThis->performance.now()).unwrap().as_f64().unwrap();
	let start = now();
	assert_eq!(timeout(sleep(500), 20).await, None);
	assert!(now() - start < 300.0);
}

fn record(
	push: &(impl Fn(String) + Clone + 'static),
	label: &'static str,
) -> impl FnMut(web_sys::Event) + 'static {
	let push = push.clone();
	move |event| push(format!("{label}:{}", event.type_()))
}

fn dispatch(target: &js::Value, event_type: &str) {
	let event =
		js::new!(globalThis->Event(event_type, js::obj! { bubbles: true })).unwrap();
	js::call!(target->dispatchEvent(event)).unwrap();
}

#[wasm_bindgen_test]
fn listener_receives_events_until_dropped() {
	let target: js::Value = document().create_element("div").unwrap().into();
	let (seen, push) = log();
	let listener =
		Listener::new(&target, "ping", ListenOptions::default(), record(&push, "a"))
			.unwrap();
	dispatch(&target, "ping");
	dispatch(&target, "other");
	assert_eq!(*seen.borrow(), vec!["a:ping"]);
	drop(listener);
	dispatch(&target, "ping");
	assert_eq!(seen.borrow().len(), 1);
}

// `Listener::drop` must pass the same `capture` flag to `removeEventListener` that was
// used with `addEventListener`.
#[wasm_bindgen_test]
fn listener_capture_runs_first_and_detaches() {
	let document = document();
	let parent: js::Value = document.create_element("div").unwrap().into();
	let child: js::Value = document.create_element("span").unwrap().into();
	js::call!(parent->appendChild(child)).unwrap();
	let (seen, push) = log();
	let capture = ListenOptions { capture: true, ..Default::default() };
	let outer =
		Listener::new(&parent, "ping", capture, record(&push, "parent")).unwrap();
	let _inner =
		Listener::new(&child, "ping", ListenOptions::default(), record(&push, "child"))
			.unwrap();
	dispatch(&child, "ping");
	assert_eq!(*seen.borrow(), vec!["parent:ping", "child:ping"]);
	drop(outer);
	dispatch(&child, "ping");
	assert_eq!(*seen.borrow(), vec!["parent:ping", "child:ping", "child:ping"]);
}

#[wasm_bindgen_test]
fn listener_once_and_detach() {
	let target: js::Value = document().create_element("div").unwrap().into();
	let (seen, push) = log();
	let once = ListenOptions { once: true, ..Default::default() };
	let _once = Listener::new(&target, "ping", once, record(&push, "once")).unwrap();
	Listener::new(&target, "ping", ListenOptions::default(), record(&push, "kept"))
		.unwrap()
		.detach();
	dispatch(&target, "ping");
	dispatch(&target, "ping");
	assert_eq!(*seen.borrow(), vec!["once:ping", "kept:ping", "kept:ping"]);
}

fn cancelled_with(passive: Option<bool>) -> bool {
	let target: js::Value = document().create_element("div").unwrap().into();
	let options = ListenOptions { passive, ..Default::default() };
	let _listener = Listener::new(&target, "ping", options, |event: web_sys::Event| {
		event.prevent_default();
	})
	.unwrap();
	let init = js::obj! { cancelable: true };
	let event = js::new!(globalThis->Event("ping", init)).unwrap();
	!js::call!(target->dispatchEvent(event)).unwrap().is_truthy()
}

#[wasm_bindgen_test]
fn listener_passive_ignores_prevent_default() {
	assert!(!cancelled_with(Some(true)));
	assert!(cancelled_with(Some(false)));
	assert!(cancelled_with(None));
}

#[wasm_bindgen_test]
fn listener_of_passes_matching_events_only() {
	let target: js::Value = document().create_element("div").unwrap().into();
	let (calls, tick) = counter();
	let (seen, push) = log();
	let _listener = Listener::of(
		&target,
		"ping",
		ListenOptions::default(),
		move |event: web_sys::MouseEvent| {
			tick();
			push(event.client_x());
		},
	)
	.unwrap();
	let mouse =
		js::new!(globalThis->MouseEvent("ping", js::obj! { clientX: 7 })).unwrap();
	js::call!(target->dispatchEvent(mouse)).unwrap();
	let plain = js::new!(globalThis->Event("ping")).unwrap();
	js::call!(target->dispatchEvent(plain)).unwrap();
	assert_eq!(calls.get(), 1);
	assert_eq!(*seen.borrow(), vec![7]);
}

// A non-matching `Event` dispatched before the first `MouseEvent` must not consume a
// `once: true` typed listener.
#[wasm_bindgen_test]
fn listener_of_once_skips_mismatched_events() {
	let target: js::Value = document().create_element("div").unwrap().into();
	let (seen, push) = log();
	let options = ListenOptions { once: true, ..Default::default() };
	let _listener =
		Listener::of(&target, "ping", options, move |event: web_sys::MouseEvent| {
			push(event.client_x());
		})
		.unwrap();
	let plain = js::new!(globalThis->Event("ping")).unwrap();
	js::call!(target->dispatchEvent(plain)).unwrap();
	js::call!(target->dispatchEvent(mouse("ping", 1))).unwrap();
	js::call!(target->dispatchEvent(mouse("ping", 2))).unwrap();
	assert_eq!(*seen.borrow(), vec![1]);
}

// `JsCast::dyn_into` checks `instanceof` against the current window's `MouseEvent`
// constructor, so an event created in a child iframe's realm is filtered out.
#[wasm_bindgen_test]
fn listener_of_skips_events_from_other_windows() {
	let document = document();
	let iframe = document.create_element("iframe").unwrap();
	document.body().unwrap().append_child(&iframe).unwrap();
	let window = js::get!(iframe->contentWindow).unwrap();
	let target: js::Value = document.create_element("div").unwrap().into();
	let (seen, push) = log();
	let _listener = Listener::of(
		&target,
		"ping",
		ListenOptions::default(),
		move |_: web_sys::MouseEvent| push(()),
	)
	.unwrap();
	let foreign = js::new!(window->MouseEvent("ping")).unwrap();
	js::call!(target->dispatchEvent(foreign)).unwrap();
	iframe.remove();
	assert!(seen.borrow().is_empty());
}

#[wasm_bindgen_test]
fn dropping_listener_releases_callback_immediately() {
	let target: js::Value = document().create_element("div").unwrap().into();
	let probe = Probe::default();
	let held = probe.clone();
	let listener = Listener::new(&target, "ping", ListenOptions::default(), move |_| {
		let _ = &held;
	})
	.unwrap();
	assert!(probe.held());
	drop(listener);
	assert!(!probe.held());
}

#[wasm_bindgen_test]
fn listener_rejects_non_event_targets() {
	let error = Listener::new(&js::obj! {}, "ping", ListenOptions::default(), |_| {})
		.unwrap_err();
	assert_eq!(error.message(), "addEventListener is not a function");
}

// Delays > `i32::MAX` are clamped rather than overflowing to 0 in `setTimeout`.
#[wasm_bindgen_test]
async fn timer_clamps_huge_delays() {
	let (count, tick) = counter();
	let timer = Timer::timeout(u32::MAX, tick).unwrap();
	sleep(30).await;
	assert_eq!(count.get(), 0);
	drop(timer);
}

#[wasm_bindgen_test]
async fn sleep_and_timeout_clamp_huge_delays() {
	assert_eq!(timeout(sleep(20), u32::MAX).await, Some(()));
	assert_eq!(timeout(sleep(u32::MAX), 20).await, None);
}

fn mouse(event_type: &str, x: i32) -> js::Value {
	js::new!(globalThis->MouseEvent(event_type, js::obj! { clientX: x })).unwrap()
}

#[wasm_bindgen_test]
async fn next_event_resolves_with_next_matching_event() {
	let target: js::Value = document().create_element("div").unwrap().into();
	let next =
		next_event::<web_sys::MouseEvent>(&target, "ping", ListenOptions::default())
			.unwrap();
	let plain = js::new!(globalThis->Event("ping")).unwrap();
	js::call!(target->dispatchEvent(plain)).unwrap();
	js::call!(target->dispatchEvent(mouse("ping", 3))).unwrap();
	js::call!(target->dispatchEvent(mouse("ping", 4))).unwrap();
	assert_eq!(next.await.client_x(), 3);
}

#[wasm_bindgen_test]
async fn next_event_waits_for_a_later_event() {
	let target: js::Value = document().create_element("div").unwrap().into();
	let next =
		next_event::<web_sys::MouseEvent>(&target, "ping", ListenOptions::default())
			.unwrap();
	let fire = target.clone();
	Timer::timeout(10, move || {
		js::call!(fire->dispatchEvent(mouse("ping", 5))).unwrap();
	})
	.unwrap()
	.detach();
	assert_eq!(timeout(next, 500).await.map(|event| event.client_x()), Some(5));
}

#[wasm_bindgen_test]
fn dropping_next_event_and_events_removes_listener() {
	let (calls, push) = log();
	let record = |action: &'static str| {
		let push = push.clone();
		js::func!(move |event_type| push(format!("{action}:{}", event_type.text())))
	};
	let target = js::obj! {
		addEventListener: record("add"),
		removeEventListener: record("remove"),
	};
	drop(next_event::<web_sys::Event>(&target, "a", ListenOptions::default()).unwrap());
	drop(events::<web_sys::Event>(&target, "b", ListenOptions::default()).unwrap());
	assert_eq!(*calls.borrow(), vec!["add:a", "remove:a", "add:b", "remove:b"]);
}

#[wasm_bindgen_test]
fn next_event_and_events_reject_non_event_targets() {
	let options = ListenOptions::default();
	assert!(next_event::<web_sys::Event>(&js::obj! {}, "ping", options).is_err());
	assert!(events::<web_sys::Event>(&js::obj! {}, "ping", options).is_err());
}

#[wasm_bindgen_test]
async fn events_buffers_in_order_and_ignores_once() {
	let target: js::Value = document().create_element("div").unwrap().into();
	let options = ListenOptions { once: true, ..Default::default() };
	let mut stream = events::<web_sys::MouseEvent>(&target, "ping", options).unwrap();
	js::call!(target->dispatchEvent(mouse("ping", 1))).unwrap();
	let plain = js::new!(globalThis->Event("ping")).unwrap();
	js::call!(target->dispatchEvent(plain)).unwrap();
	js::call!(target->dispatchEvent(mouse("ping", 2))).unwrap();
	assert_eq!(stream.next().await.map(|event| event.client_x()), Some(1));
	assert_eq!(stream.next().await.map(|event| event.client_x()), Some(2));
	let fire = target.clone();
	Timer::timeout(10, move || {
		js::call!(fire->dispatchEvent(mouse("ping", 3))).unwrap();
	})
	.unwrap()
	.detach();
	let third = timeout(stream.next(), 500).await.flatten();
	assert_eq!(third.map(|event| event.client_x()), Some(3));
}

#[wasm_bindgen_test]
async fn frames_yield_increasing_timestamps() {
	let mut frames = frames().unwrap();
	let mut times = Vec::new();
	for _ in 0..3 {
		times.push(timeout(frames.next(), 500).await.flatten().unwrap());
	}
	assert!(times.windows(2).all(|pair| pair[0] < pair[1]), "{times:?}");
}

// `Frames::poll_next` yields the ready timestamp before requesting the next animation
// frame so the caller's loop body executes before the next frame is scheduled.
#[wasm_bindgen_test]
async fn frames_run_the_loop_body_before_the_next_frame() {
	let first = Rc::new(Cell::new(None));
	let next_frame = Rc::new(Cell::new(false));
	let (seen, later) = (first.clone(), next_frame.clone());
	Timer::frame(move |time| {
		seen.set(Some(time));
		Timer::frame(move |_| later.set(true)).unwrap().detach();
	})
	.unwrap()
	.detach();
	let mut frames = frames().unwrap();
	let time = timeout(frames.next(), 500).await.flatten();
	assert!(!next_frame.get());
	assert_eq!(time, first.get());
}

#[wasm_bindgen_test]
fn dropping_frames_cancels_the_pending_request() {
	let original = js::get!(globalThis->cancelAnimationFrame).unwrap();
	let (ids, push) = log();
	let real = original.clone();
	let spy = js::func!(move |id| -> js::Result {
		push(id.clone());
		js::call!(real->call(js::Value::UNDEFINED, id))
	});
	let stream = frames().unwrap();
	js::set!(globalThis->cancelAnimationFrame = spy).unwrap();
	drop(stream);
	js::set!(globalThis->cancelAnimationFrame = original).unwrap();
	assert_eq!(ids.borrow().len(), 1);
	assert!(ids.borrow()[0].as_f64().is_some());
}

#[wasm_bindgen_test]
async fn frames_error_and_end_without_request_animation_frame() {
	let mut stream = frames().unwrap();
	timeout(stream.next(), 500).await.flatten().unwrap();
	let original = js::get!(globalThis->requestAnimationFrame).unwrap();
	js::set!(globalThis->requestAnimationFrame = js::Value::UNDEFINED).unwrap();
	let created = frames().map(drop);
	let polled = stream.next().now_or_never();
	js::set!(globalThis->requestAnimationFrame = original).unwrap();
	assert!(created.is_err());
	assert_eq!(polled, Some(None));
}
