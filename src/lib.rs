#![deny(missing_docs)]
#![cfg_attr(feature = "cargo-clippy", feature(tool_lints))]
#![cfg_attr(feature = "cargo-clippy", warn(clippy::all))]
//! # Ruukh - Introduction
//!
//! Welcome to Ruukh, the frontend web framework.
//!
//! This API reference tries to be both helpful for the users as well as
//! anybody who wants to understand how this framework works. So, if you find
//! anything you do not understand or is wrong here, feel free to open an
//! issue/PR at [github](https://github.com/csharad/ruukh).
//!
//! To create an app, you must first implement a root component which neither
//! accepts props nor accepts events. This component is then mounted on a DOM
//! node like so:
//!
//! # Example
//! ```
//! use ruukh::prelude::*;
//! use wasm_bindgen::prelude::*;
//!
//! #[component]
//! #[derive(Lifecycle)]
//! struct MyApp;
//!
//! impl Render for MyApp {
//!     fn render(&self) -> Markup<Self> {
//!         html! {
//!             Hello World!
//!         }
//!     }
//! }
//!
//! #[wasm_bindgen]
//! pub fn run() -> ReactiveApp {
//!     App::<MyApp>::new().mount("app")
//! }
//! ```
//!
//! Here, "app" is the `id` of an element where you want to mount the App.
//!
//! Note: Docs on macros are located [here](../../ruukh_codegen/index.html).

extern crate fnv;
extern crate indexmap;
extern crate ruukh_codegen;
extern crate wasm_bindgen;
#[cfg(test)]
extern crate wasm_bindgen_test;
#[cfg(test)]
use wasm_bindgen_test::*;

#[cfg(test)]
wasm_bindgen_test_configure!(run_in_browser);

use component::{Render, RootParent};
use std::cell::{Ref, RefCell, RefMut};
use std::rc::Rc;
use vdom::vcomponent::{ComponentManager, ComponentWrapper};
use wasm_bindgen::prelude::*;
use web_api::*;

pub mod component;
mod dom;
pub mod vdom;
#[cfg_attr(feature = "cargo-clippy", allow(clippy::all))]
pub mod web_api;

/// A VDOM Markup which is generated by using `html!` macro.
pub type Markup<RCTX> = vdom::VNode<RCTX>;

/// Things you'll require to build the next great App. Just glob import the
/// prelude and start building your app.
pub mod prelude {
    pub use component::{Component, Lifecycle, Render};
    pub use ruukh_codegen::*;
    pub use Markup;
    pub use {App, ReactiveApp};
}

/// Things the proc-macro uses without bugging the using to import them.
pub mod reexports {
    pub use fnv::FnvBuildHasher;
    pub use indexmap::IndexMap;
}

/// The main entry point to use your component and run it on the browser.
pub struct App<COMP>
where
    COMP: Render<Props = (), Events = ()>,
{
    manager: ComponentWrapper<COMP, RootParent>,
}

impl<COMP> App<COMP>
where
    COMP: Render<Props = (), Events = ()>,
{
    /// Create a new App with a `Component` struct passed as its type parameter.
    ///
    /// The component that is mounted as an App should not have any props and
    /// events declared onto it.
    ///
    /// # Example
    /// ```ignore
    /// let my_app = App::<MyApp>::new();
    /// ```
    pub fn new() -> App<COMP> {
        Default::default()
    }

    /// Mounts the app on the given element in the DOM.
    ///
    /// The element may be anything that implements
    /// [AppMount](trait.AppMount.html). You may pass an id of an element
    /// or an element node itself.
    ///
    /// # Example
    /// ```ignore
    /// App::<MyApp>::new().mount("app")
    /// ```
    ///
    /// Note:
    /// Be sure to return the [ReactiveApp](struct.ReactiveApp.html) to the
    /// JS side because we want our app to live for 'static lifetimes (i.e.
    /// As long as the browser/tab runs).
    pub fn mount<E: AppMount>(mut self, element: E) -> ReactiveApp {
        let parent = element.app_mount();
        let (mut channel, sender) = ReactiveApp::new();

        // Every component requires a render context, so provided a void context.
        let root_parent = Shared::new(());

        // The first render
        self.manager
            .render_walk(parent.as_ref(), None, root_parent.clone(), sender.clone())
            .unwrap();

        // Rerender when it receives update messages.
        channel.on_message(move || {
            self.manager
                .render_walk(parent.as_ref(), None, root_parent.clone(), sender.clone())
                .unwrap();
        });

        channel
    }
}

impl<COMP> Default for App<COMP>
where
    COMP: Render<Props = (), Events = ()>,
{
    /// Create a new App with a component `COMP` that has void props and events.
    fn default() -> Self {
        App {
            manager: ComponentWrapper::new((), ()),
        }
    }
}

/// This is a mounted app which reacts to state changes and rerenders itself.
///
/// ## Internals
///
/// It stores the receiver end of the message port which listens to any
/// messages passed from the sender end
/// ([MessageSender](struct.MessageSender.html)), which itself is stored
/// within each of the component's status. Whenever a component changes it
/// state, it sends an update message via the MessageSender to which the
/// listener reacts by rerendering the App.
#[wasm_bindgen]
pub struct ReactiveApp {
    rx: MessagePort,
    on_message: Option<Closure<FnMut(JsValue)>>,
}

impl ReactiveApp {
    /// Create new a reactive app.
    fn new() -> (ReactiveApp, MessageSender) {
        let msg_channel = MessageChannel::new();
        (
            ReactiveApp {
                rx: msg_channel.port2(),
                on_message: None,
            },
            MessageSender {
                tx: msg_channel.port1(),
            },
        )
    }

    /// Invokes the handler, when it receives a message.
    fn on_message<F: FnMut() + 'static>(&mut self, mut handler: F) {
        let closure: Closure<FnMut(JsValue)> = Closure::wrap(Box::new(move |_| handler()));
        self.rx.on_message(&closure);
        self.on_message = Some(closure);
    }
}

/// MessageSender is responsible to message the App about state changes.
#[derive(Clone)]
struct MessageSender {
    tx: MessagePort,
}

impl MessageSender {
    /// Send an update message to the [App](struct.App.html).
    ///
    /// The components need to call this method, when it desires the app to
    /// be notified of state changes.
    fn do_react(&self) {
        // Just send a `null` as we have only a single message to be sent.
        self.tx
            .post_message(&JsValue::null())
            .expect("Could not send the message");
    }
}

/// A Shared Value.
///
/// ## Internals
///
/// Writing `Rc::new(RefCell::new(val))` is tedious.
pub struct Shared<T>(Rc<RefCell<T>>);

impl<T> Shared<T> {
    /// Create a new Shared value.
    fn new(val: T) -> Shared<T> {
        Shared(Rc::new(RefCell::new(val)))
    }

    /// Borrows the inner value.
    pub fn borrow(&self) -> Ref<T> {
        self.0.borrow()
    }

    /// Borrows the inner value mutably.
    pub fn borrow_mut(&self) -> RefMut<T> {
        self.0.borrow_mut()
    }
}

impl<T> Clone for Shared<T> {
    fn clone(&self) -> Self {
        Shared(self.0.clone())
    }
}

/// Trait to get an element on which the App is going to be mounted.
pub trait AppMount {
    /// Consumes `self` and gets an element from the DOM.
    ///
    /// If the implementation returns an error, panic it instead as it is not
    /// worth it to run the app anymore.
    fn app_mount(self) -> Element;
}

impl<'a> AppMount for &'a str {
    fn app_mount(self) -> Element {
        html_document.get_element_by_id(self).unwrap_or_else(|| {
            panic!(
                "Could not find element with id `{}` to mount the App.",
                self
            )
        })
    }
}

impl AppMount for Element {
    fn app_mount(self) -> Element {
        self
    }
}

impl AppMount for String {
    fn app_mount(self) -> Element {
        self.as_str().app_mount()
    }
}

/// For use in tests.
#[cfg(test)]
fn message_sender() -> MessageSender {
    ReactiveApp::new().1
}
