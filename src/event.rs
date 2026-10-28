//! Timers, animation-frame streams, and DOM `EventTarget` listeners (`event` feature).

use std::cell::RefCell;
use std::collections::VecDeque;
use std::future::Future;
use std::pin::{Pin, pin};
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

use crate::{self as js, Result, Value};
use futures_util::Stream;
use futures_util::future::{Either, select};
use gloo_timers::future::TimeoutFuture;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::Closure;

/// A guard for a scheduled `setTimeout`, `setInterval`, or `requestAnimationFrame`
/// callback.
///
/// Dropping a `Timer` cancels the underlying JS timer or animation-frame request
/// (`clearTimeout`, `clearInterval`, or `cancelAnimationFrame`) and immediately drops
/// the Rust closure and its captured state. Call `Timer::detach` to keep the timer
/// running after the handle is dropped.
///
/// # Examples
///
/// ```no_run
/// use js::Timer;
///
/// # fn example() -> js::Result<()> {
/// let timer = Timer::timeout(100, || {})?;
/// drop(timer); // cancelled before firing
///
/// Timer::timeout(100, || {})?.detach(); // runs in the background
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
#[must_use = "dropping a timer cancels it at once; call `detach` to keep it running"]
pub struct Timer {
	id: Value,
	kind: Kind,
	callback: Option<Closure<dyn FnMut(Value)>>,
}

#[derive(Clone, Copy, Debug)]
enum Kind {
	Timeout,
	Interval,
	Frame,
}

impl Kind {
	fn start(self, function: &Value, ms: u32) -> Result {
		let ms = delay(ms);
		match self {
			Self::Timeout => js::call!(globalThis->setTimeout(function, ms)),
			Self::Interval => js::call!(globalThis->setInterval(function, ms)),
			Self::Frame => js::call!(globalThis->requestAnimationFrame(function)),
		}
	}

	fn cancel(self, id: &Value) -> Result {
		match self {
			Self::Timeout => js::call!(globalThis->clearTimeout(id)),
			Self::Interval => js::call!(globalThis->clearInterval(id)),
			Self::Frame => js::call!(globalThis->cancelAnimationFrame(id)),
		}
	}
}

// Browser timer APIs take a signed 32-bit delay (`i32`) and treat values above
// `i32::MAX` as 0 due to 32-bit overflow. Clamping to `i32::MAX` (~24.8 days) avoids
// firing immediately on large `u32` inputs.
fn delay(ms: u32) -> i32 {
	i32::try_from(ms).unwrap_or(i32::MAX)
}

impl Timer {
	/// Schedules `callback` to run once after `ms` milliseconds via
	/// `globalThis.setTimeout`.
	///
	/// Delays above `i32::MAX` (`2_147_483_647` ms) are clamped to `i32::MAX`.
	///
	/// # Errors
	///
	/// Returns `Err` if `globalThis.setTimeout` is missing or throws.
	///
	/// # Examples
	///
	/// ```no_run
	/// use js::Timer;
	///
	/// # fn example() -> js::Result<()> {
	/// let _timer = Timer::timeout(50, || {})?;
	/// # Ok(())
	/// # }
	/// ```
	pub fn timeout(ms: u32, callback: impl FnOnce() + 'static) -> Result<Self> {
		Self::start(Kind::Timeout, ms, Closure::once(move |_: Value| callback()))
	}

	/// Schedules `callback` to run repeatedly every `ms` milliseconds via
	/// `globalThis.setInterval` until the returned `Timer` is dropped.
	///
	/// Delays above `i32::MAX` (`2_147_483_647` ms) are clamped to `i32::MAX`.
	///
	/// # Errors
	///
	/// Returns `Err` if `globalThis.setInterval` is missing or throws.
	///
	/// # Examples
	///
	/// ```no_run
	/// use js::Timer;
	///
	/// # fn example() -> js::Result<()> {
	/// let _interval = Timer::interval(100, || {})?;
	/// # Ok(())
	/// # }
	/// ```
	pub fn interval(ms: u32, mut callback: impl FnMut() + 'static) -> Result<Self> {
		Self::start(Kind::Interval, ms, Closure::new(move |_: Value| callback()))
	}

	/// Schedules `callback` to run on the next repaint via
	/// `globalThis.requestAnimationFrame`, passing the `DOMHighResTimeStamp` in
	/// milliseconds.
	///
	/// Requires a rendering document environment that provides
	/// `globalThis.requestAnimationFrame` (not available in Node.js or non-rendering
	/// workers).
	///
	/// # Errors
	///
	/// Returns `Err` if `globalThis.requestAnimationFrame` is unavailable or throws.
	///
	/// # Examples
	///
	/// ```no_run
	/// use js::Timer;
	///
	/// # fn example() -> js::Result<()> {
	/// let _frame = Timer::frame(|_timestamp| {})?;
	/// # Ok(())
	/// # }
	/// ```
	pub fn frame(callback: impl FnOnce(f64) + 'static) -> Result<Self> {
		let callback = Closure::once(move |time: Value| {
			callback(time.as_f64().unwrap_or_default());
		});
		Self::start(Kind::Frame, 0, callback)
	}

	fn start(kind: Kind, ms: u32, callback: Closure<dyn FnMut(Value)>) -> Result<Self> {
		let id = kind.start(callback.as_ref(), ms)?;
		Ok(Self { id, kind, callback: Some(callback) })
	}

	/// Consumes the `Timer` without cancelling it, handing the closure to the JS garbage
	/// collector so the timer continues to run on its own.
	pub fn detach(mut self) {
		if let Some(callback) = self.callback.take() {
			drop(callback.into_js_value());
		}
	}
}

impl Drop for Timer {
	fn drop(&mut self) {
		if self.callback.is_some() {
			let _ = self.kind.cancel(&self.id);
		}
	}
}

/// Waits for `ms` milliseconds using `setTimeout`.
///
/// Delays above `i32::MAX` (`2_147_483_647` ms) are clamped to `i32::MAX`.
///
/// # Examples
///
/// ```no_run
/// # async fn example() {
/// js::sleep(10).await;
/// # }
/// ```
pub async fn sleep(ms: u32) {
	TimeoutFuture::new(delay(ms).unsigned_abs()).await;
}

/// Runs `future` with a deadline of `ms` milliseconds, returning `Some(output)` if it
/// completes in time or `None` if the timeout elapses first.
///
/// Delays above `i32::MAX` (`2_147_483_647` ms) are clamped to `i32::MAX`.
///
/// # Examples
///
/// ```no_run
/// # async fn example() {
/// assert_eq!(js::timeout(async { 42 }, 100).await, Some(42));
/// assert_eq!(js::timeout(js::sleep(500), 10).await, None);
/// # }
/// ```
pub async fn timeout<T>(future: impl Future<Output = T>, ms: u32) -> Option<T> {
	match select(pin!(future), pin!(sleep(ms))).await {
		Either::Left((value, _)) => Some(value),
		Either::Right(_) => None,
	}
}

/// Creates a `Stream` of animation-frame timestamps (`f64` milliseconds) driven by
/// `globalThis.requestAnimationFrame`.
///
/// Requests the first frame immediately and schedules each subsequent frame only when
/// the stream is polled after yielding the previous one. Requires a rendering document
/// environment that provides `globalThis.requestAnimationFrame`.
///
/// # Errors
///
/// Returns `Err` if `globalThis.requestAnimationFrame` is unavailable or throws when
/// requesting the initial frame.
///
/// # Examples
///
/// ```no_run
/// use futures_util::StreamExt as _;
/// # async fn example() -> js::Result<()> {
/// let mut frames = js::frames()?;
/// if let Some(timestamp) = frames.next().await {
///     assert!(timestamp >= 0.0);
/// }
/// # Ok(())
/// # }
/// ```
pub fn frames() -> Result<Frames> {
	let mut frames = Frames { queue: Queue::shared(), pending: None, ended: false };
	frames.request()?;
	Ok(frames)
}

/// A `Stream` of `DOMHighResTimeStamp` values (`f64`) produced by `frames()`.
///
/// Dropping `Frames` cancels any in-flight `requestAnimationFrame` callback. If
/// `requestAnimationFrame` becomes unavailable on a later poll, the stream terminates
/// with `None`.
#[must_use = "streams do nothing unless polled; dropping this cancels the request"]
pub struct Frames {
	queue: Rc<RefCell<Queue<f64>>>,
	pending: Option<Timer>,
	ended: bool,
}

impl Frames {
	fn request(&mut self) -> Result<()> {
		let sink = self.queue.clone();
		self.pending = Some(Timer::frame(move |time| Queue::push(&sink, time))?);
		Ok(())
	}
}

impl Stream for Frames {
	type Item = f64;

	fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<f64>> {
		let this = self.get_mut();
		if this.ended {
			return Poll::Ready(None);
		}
		// Request the next frame only when polled again after yielding, so frames that
		// pass while the caller's loop body runs are skipped rather than queued.
		let polled = this.queue.borrow_mut().poll(cx);
		if let Poll::Ready(time) = polled {
			this.pending = None;
			return Poll::Ready(Some(time));
		}
		if this.pending.is_none() && this.request().is_err() {
			this.ended = true;
			return Poll::Ready(None);
		}
		Poll::Pending
	}
}

/// An active DOM `addEventListener` registration that removes itself on drop.
///
/// Dropping a `Listener` calls `removeEventListener` on the target and immediately
/// releases the Rust closure and its captures. Call `Listener::detach` to keep the
/// listener attached permanently.
///
/// # Examples
///
/// ```no_run
/// use js::{ListenOptions, Listener, Value};
///
/// # fn example(target: &Value) -> js::Result<()> {
/// let listener = Listener::new(target, "click", ListenOptions::default(), |_ev| {})?;
/// drop(listener); // removed from `target`
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
#[must_use = "dropping a listener removes it; call `detach` to keep it attached"]
pub struct Listener {
	target: Value,
	event_type: String,
	capture: bool,
	callback: Option<Closure<dyn FnMut(web_sys::Event)>>,
}

/// Options passed to `addEventListener` via `Listener`, `next_event`, and `events`.
#[derive(Clone, Copy, Debug, Default)]
pub struct ListenOptions {
	/// Dispatch during the capture phase (`capture: true`) rather than bubbling.
	pub capture: bool,
	/// Invoke the callback at most once (`once: true`). Ignored by `events`, which is
	/// always a multi-event stream.
	pub once: bool,
	/// Whether the listener never calls `preventDefault()` (`passive`). When `None`,
	/// the browser's default for the event type and target is used.
	pub passive: Option<bool>,
}

impl Listener {
	/// Attaches an untyped `web_sys::Event` listener for `event_type` on `target`.
	///
	/// # Errors
	///
	/// Returns `Err` if `target` does not implement `addEventListener` or if
	/// registration throws.
	///
	/// # Examples
	///
	/// ```no_run
	/// use js::{ListenOptions, Listener, Value};
	///
	/// # fn example(target: &Value) -> js::Result<()> {
	/// let options = ListenOptions::default();
	/// let _listener = Listener::new(target, "click", options, |_e| {})?;
	/// assert!(Listener::new(&Value::NULL, "click", options, |_| {}).is_err());
	/// # Ok(())
	/// # }
	/// ```
	pub fn new(
		target: &Value,
		event_type: &str,
		options: ListenOptions,
		callback: impl FnMut(web_sys::Event) + 'static,
	) -> Result<Self> {
		Self::register(target, event_type, options, Closure::new(callback))
	}

	/// Attaches a typed event listener that filters incoming events to `E` via
	/// `JsCast::dyn_into`.
	///
	/// Events that are not instances of `E` in the current window/realm (for example,
	/// a plain `Event` when listening for `MouseEvent`, or an event constructed in a
	/// different `iframe` window) are silently ignored. Every event on a target inside
	/// another window is created with that window's constructors, so it never matches.
	/// When `options.once` is `true`, the one-shot latch is tracked in Rust so ignored
	/// non-matching events do not consume the listener before the first `E` arrives.
	///
	/// # Errors
	///
	/// Returns `Err` if `target` does not implement `addEventListener` or if
	/// registration throws.
	///
	/// # Examples
	///
	/// ```no_run
	/// use js::{ListenOptions, Listener, Value};
	///
	/// # fn example(target: &Value) -> js::Result<()> {
	/// let _listener = Listener::of(
	///     target,
	///     "click",
	///     ListenOptions::default(),
	///     |event: web_sys::MouseEvent| {
	///         let _ = event.client_x();
	///     },
	/// )?;
	/// # Ok(())
	/// # }
	/// ```
	pub fn of<E: JsCast + 'static>(
		target: &Value,
		event_type: &str,
		options: ListenOptions,
		mut callback: impl FnMut(E) + 'static,
	) -> Result<Self> {
		let once = options.once;
		let mut done = false;
		let callback = Closure::new(move |event: web_sys::Event| {
			if done {
				return;
			}
			if let Ok(event) = event.dyn_into::<E>() {
				done = once;
				callback(event);
			}
		});
		// Pass `once: false` to JS so a non-`E` event fired on the same `event_type`
		// does not cause the browser to remove the listener early.
		let options = ListenOptions { once: false, ..options };
		Self::register(target, event_type, options, callback)
	}

	fn register(
		target: &Value,
		event_type: &str,
		options: ListenOptions,
		callback: Closure<dyn FnMut(web_sys::Event)>,
	) -> Result<Self> {
		let function: &Value = callback.as_ref();
		let ListenOptions { capture, once, passive } = options;
		let init = js::obj! { capture: capture, once: once };
		if let Some(passive) = passive {
			js::set!(init->passive = passive)?;
		}
		js::call!(target->addEventListener(event_type, function, init))?;
		Ok(Self {
			target: target.clone(),
			event_type: event_type.to_owned(),
			capture,
			callback: Some(callback),
		})
	}

	/// Consumes the `Listener` without calling `removeEventListener`, handing the
	/// closure to the JS garbage collector so the listener remains attached for the
	/// lifetime of `target`.
	pub fn detach(mut self) {
		if let Some(callback) = self.callback.take() {
			drop(callback.into_js_value());
		}
	}
}

impl Drop for Listener {
	fn drop(&mut self) {
		let Some(callback) = &self.callback else { return };
		let (target, event_type, capture) =
			(&self.target, &self.event_type, self.capture);
		let function: &Value = callback.as_ref();
		let _ = js::call!(target->removeEventListener(event_type, function, capture));
	}
}

fn listen<E: JsCast + 'static>(
	target: &Value,
	event_type: &str,
	options: ListenOptions,
) -> Result<(Rc<RefCell<Queue<E>>>, Listener)> {
	let queue = Queue::shared();
	let sink = queue.clone();
	let listener = Listener::of(target, event_type, options, move |event| {
		Queue::push(&sink, event)
	})?;
	Ok((queue, listener))
}

/// Attaches a one-shot listener on `target` and returns a `Future` that resolves to
/// the next matching event of type `E`.
///
/// The listener is registered immediately (so events dispatched before the future is
/// first polled are still captured) and is removed automatically when the returned
/// `NextEvent` is dropped.
///
/// # Errors
///
/// Returns `Err` if `target` does not implement `addEventListener` or if registration
/// throws.
///
/// # Examples
///
/// ```no_run
/// use js::{ListenOptions, Value, next_event};
///
/// # async fn example(target: &Value) -> js::Result<()> {
/// let event: web_sys::Event =
///     next_event(target, "click", ListenOptions::default())?.await;
/// assert_eq!(event.type_(), "click");
/// # Ok(())
/// # }
/// ```
pub fn next_event<E: JsCast + 'static>(
	target: &Value,
	event_type: &str,
	options: ListenOptions,
) -> Result<NextEvent<E>> {
	let options = ListenOptions { once: true, ..options };
	let (queue, listener) = listen(target, event_type, options)?;
	Ok(NextEvent { queue, _listener: listener })
}

/// Future returned by `next_event` that resolves to the next matching event `E` and
/// removes its listener when dropped.
#[must_use = "futures do nothing unless awaited; dropping this removes the listener"]
pub struct NextEvent<E> {
	queue: Rc<RefCell<Queue<E>>>,
	_listener: Listener,
}

impl<E> Future for NextEvent<E> {
	type Output = E;

	fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<E> {
		self.queue.borrow_mut().poll(cx)
	}
}

/// Attaches a listener on `target` and returns an unbounded `Stream` of matching
/// events of type `E` in dispatch order.
///
/// `options.once` is ignored (`once: false` is always used). Dropping the returned
/// `Events` stream removes the underlying listener from `target`.
///
/// # Errors
///
/// Returns `Err` if `target` does not implement `addEventListener` or if registration
/// throws.
///
/// # Examples
///
/// ```no_run
/// use futures_util::StreamExt as _;
/// use js::{ListenOptions, Value, events};
///
/// # async fn example(target: &Value) -> js::Result<()> {
/// let options = ListenOptions::default();
/// let mut clicks = events::<web_sys::Event>(target, "click", options)?;
/// if let Some(event) = clicks.next().await {
///     assert_eq!(event.type_(), "click");
/// }
/// # Ok(())
/// # }
/// ```
pub fn events<E: JsCast + 'static>(
	target: &Value,
	event_type: &str,
	options: ListenOptions,
) -> Result<Events<E>> {
	let options = ListenOptions { once: false, ..options };
	let (queue, listener) = listen(target, event_type, options)?;
	Ok(Events { queue, _listener: listener })
}

/// Stream returned by `events` that yields matching events `E` until dropped.
#[must_use = "streams do nothing unless polled; dropping this removes the listener"]
pub struct Events<E> {
	queue: Rc<RefCell<Queue<E>>>,
	_listener: Listener,
}

impl<E> Stream for Events<E> {
	type Item = E;

	fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<E>> {
		self.queue.borrow_mut().poll(cx).map(Some)
	}
}

// Single-consumer FIFO queue shared between a JS callback and a `Future` or `Stream`.
struct Queue<T> {
	items: VecDeque<T>,
	waker: Option<Waker>,
}

impl<T> Queue<T> {
	fn shared() -> Rc<RefCell<Self>> {
		Rc::new(RefCell::new(Self { items: VecDeque::new(), waker: None }))
	}

	fn push(queue: &RefCell<Self>, item: T) {
		// Release the `RefMut` borrow before waking the task in case the waker polls
		// synchronously on the same call stack.
		let waker = {
			let mut queue = queue.borrow_mut();
			queue.items.push_back(item);
			queue.waker.take()
		};
		if let Some(waker) = waker {
			waker.wake();
		}
	}

	fn poll(&mut self, cx: &Context<'_>) -> Poll<T> {
		match self.items.pop_front() {
			Some(item) => Poll::Ready(item),
			None => {
				self.waker = Some(cx.waker().clone());
				Poll::Pending
			}
		}
	}
}
