//! Graphical User Interface for Rust using HTML code to create visual components and to build
//! UI from them. Also, attachment of CSS and JS is possible. This library
//! also uses `webview` crate to display the content and control the page as components
//! get modified by the user actions or by Rust back-end.

#[macro_use]
extern crate typed_html;
extern crate web_view;
extern crate serde_derive;
extern crate serde_json;
pub extern crate htmldom_read;
extern crate owning_ref;
extern crate rsgen;
extern crate base64;
extern crate uitaco_derive;

pub use uitaco_derive::*;

use serde_derive::{Deserialize};
use web_view::{Content, WVResult};
use std::sync::{Arc, RwLock, mpsc, Weak};
use std::collections::{HashMap, HashSet};
use crate::component::{ComponentBase, ComponentHandle, ComponentId, Component, Container, AddComponentError, ChildrenLogic, ChildrenLogicAddError, ClassHandle};
use typed_html::dom::DOMTree;
use crate::tags::{Element, TagName};
use htmldom_read::Node;
use std::fmt::{Debug, Formatter};
pub use owning_ref::{RwLockReadGuardRef, RwLockWriteGuardRefMut};
use std::thread;

/// Components allow to build user interface using repeated patterns with binding to elements.
/// This allows to speed up building of UI. Binding allows to easily access contents from Rust.
pub mod component;

/// Tags that are used to access corresponding information of the page.
pub mod tags;

/// Events that can be generated by tags.
pub mod events;

/// Allows to format JS-strings prefixing quote signs if present with `\`.
/// For example string `elementById("")` will be transformed to `elementById(\"\")`.
pub fn js_prefix_quotes(s: &str) -> String {
    let mut quote_count = 0;
    for c in s.chars() {
        if c == '\'' || c == '"' {
            quote_count += 1;
        }
    }

    let mut new_s = String::with_capacity(s.len() + quote_count);
    for c in s.chars() {
        if c == '\'' || c == '"' {
            new_s.push('\\');
        }
        new_s.push(c);
    }

    new_s
}

/// Root component must be added first.
const ROOT_COMPONENT_ID: ComponentId = 0;

type UserData = Vec<(String, String)>;
//type WebView<'a> = _WebView<'a, UserData>;
type Callback = Fn(ViewHandle, String);
type RequestId = usize;
type CallbackId = usize;
type ViewId = usize;
pub type ViewHandle = Arc<RwLock<View>>;
pub type ViewWeak = Weak<RwLock<View>>;
pub type ViewGuard<'a> = RwLockReadGuardRef<'a, View>;
pub type ViewGuardMut<'a> = RwLockWriteGuardRefMut<'a, View>;

/// Command that can be sent to View.
enum ViewCmd {

    /// Evaluate given JS code.
    Eval(Option<mpsc::Sender<WVResult>>, String),
    InjectCss(String),
}

/// Wrapped instance of WebView. Also is connected to thread that runs GUI on that instance.
/// Is used to dispatch commands over GUI.
pub struct View {
    id: ViewId,
    tx: mpsc::Sender<ViewCmd>,

    // After initialization is always set to Some.
    this: Option<ViewWeak>,

    next_component_id: ComponentId,
    components: HashMap<ComponentId, Arc<RwLock<Box<dyn Component>>>>,

    next_callback_id: CallbackId,
    callbacks: HashMap<CallbackId, &'static dyn Fn(ViewHandle, String)>,

    next_request_id: RequestId,
    requests: HashMap<RequestId, mpsc::Sender<ResponseValue>>,
}

/// Wrap over view handle to make access easier.
#[derive(Clone, Debug)]
pub struct ViewWrap {
    inner: ViewHandle,
}

unsafe impl Sync for View {}
unsafe impl Send for View {}

#[derive(Clone, Debug)]
pub struct ViewBuilder {
    debug: bool,
    fullscreen: bool,
    resizable: bool,
    width: usize,
    height: usize,
    title: Option<String>,
}

#[derive(Debug)]
struct RequestBuilder {
    view: ViewHandle,
    id: RequestId,
    js: Option<String>,
    rx: mpsc::Receiver<ResponseValue>,
    tx: mpsc::Sender<ResponseValue>,
}

#[derive(Debug)]
pub struct RootComponent {
    base: ComponentBase,
}

impl Debug for View {

    fn fmt(&self, fmt: &mut Formatter) -> std::fmt::Result {
        #[derive(Debug)]
        struct DebuggableView<'a> {
            id: ViewId,
            tx: &'a mpsc::Sender<ViewCmd>,

            next_component_id: ComponentId,
            components: &'a HashMap<ComponentId, Arc<RwLock<Box<dyn Component>>>>,

            next_callback_id: CallbackId,
            callbacks: HashSet<CallbackId>,

            next_request_id: RequestId,
            requests: &'a HashMap<RequestId, mpsc::Sender<ResponseValue>>,
        };

        let callbacks = {
            let mut set
                = HashSet::with_capacity(self.callbacks.len());

            for (i, _) in &self.callbacks {
                set.insert(*i);
            }

            set
        };

        let s = DebuggableView {
            id: self.id,
            tx: &self.tx,

            next_component_id: self.next_component_id,
            components: &self.components,

            next_callback_id: self.next_callback_id,
            callbacks,

            next_request_id: self.next_request_id,
            requests: &self.requests,
        };

        s.fmt(fmt)
    }
}

impl View {

    /// Get new builder to help creating view.
    pub fn new_builder() -> ViewBuilder {
        ViewBuilder {
            debug: true,
            fullscreen: false,
            resizable: true,
            width: 640,
            height: 480,
            title: None,
        }
    }

    /// Create new view. This opens a WebView window.
    pub fn new_from_builder(builder: ViewBuilder) -> ViewWrap {
        let mut my_builder = web_view::builder();
        my_builder.debug = builder.debug;
        my_builder.resizable = builder.resizable;
        my_builder.title = "Unnamed Uitaco";
        my_builder.width = builder.width as _;
        my_builder.height = builder.height as _;

        let uitaco_body_id = "uitacoBody";

        let content = {
            let uitaco_body_id = typed_html::types::Id::new(uitaco_body_id);
            let i: DOMTree<String> = html!(
                <html>
                <head><title /></head>
                <body class=component::COMPONENT_MARK id=uitaco_body_id></body>
                </html>
            );
            i.to_string()
        };

        my_builder.content = Some(Content::Html(content));

        let (tx, rx) = mpsc::channel();
        let view = View {
            id: 0,
            tx,

            this: None,

            next_component_id: 0,
            components: Default::default(),

            next_request_id: 0,
            requests: Default::default(),

            next_callback_id: 0,
            callbacks: Default::default(),
        };

        let arc = Arc::new(RwLock::new(view));
        let arc2 = arc.clone();
        let mut view = arc2.write().unwrap();
        view.this = Some(Arc::downgrade(&arc));

        let arc2 = arc.clone();

        // Thread where WebView will live.
        thread::spawn(move || {
            let mut webview = my_builder
                .invoke_handler(move |_view, arg| {
                    let mut view = arc2.write().unwrap();
                    view.handler(arg)
                })
                .user_data(UserData::new())
                .build().unwrap();

            loop {
                let result = rx.recv();
                if let Err(_) = result {
                    break; // TODO notify any who waits for this death.
                }

                use ViewCmd::*;
                match result.unwrap() {
                    Eval(ref sender, ref st) => {
                        let result = webview.eval(st);
                        if let Some(sender) = sender {
                            sender.send(result).unwrap();
                        }
                    },
                    InjectCss(ref st) => {
                        let result = webview.inject_css(st);
                        if let Err(_) = result {
                            // Nothing.
                        }
                    }
                }
            }
        });

        ViewWrap { inner: arc }
    }

    /// Get new handle on this view.
    pub fn handle(&self) -> ViewHandle {
        self.this.as_ref().unwrap().upgrade().unwrap()
    }

    /// Get access to root component Arc.
    pub fn root_component(&mut self) -> ComponentHandle {
        ComponentHandle::new(self.handle(), ROOT_COMPONENT_ID)
    }

    /// Add new component to the interface and get a handle to it.
    fn add_component(&mut self, component: Box<dyn Component>) -> ComponentHandle {
        let id = {
            let id = self.next_component_id;
            self.next_component_id += 1;

            self.components.insert(id, Arc::new(RwLock::new(component)));
            id
        };

        ComponentHandle::new(self.handle(), id)
    }

    /// Try removing component from the interface. If it does not exist None is returned.
    /// Also, it may still be in use though it will still be removed from the interface
    /// and all new changes to the component will be therefore ignored.
    fn remove_component(&mut self, handle: &ComponentHandle) -> Option<()> {
        let option = self.components.remove(&handle.id());
        if let Some(_) = option {
            Some(())
        } else {
            None
        }
    }

    /// Inject styles to the view.
    pub fn inject_css(&mut self, css: String) {
        self.tx.send(ViewCmd::InjectCss(css)).unwrap();
    }

    /// Run given JS code and wait for result.
    pub fn eval_wait(&mut self, js: String) -> WVResult {
        let (tx, rx) = mpsc::channel();
        self.tx.send(ViewCmd::Eval(Some(tx), js)).unwrap();
        let recv = rx.recv().unwrap();
        recv
    }

    /// Run given JS code without waiting for result.
    pub fn eval(&mut self, js: String) {
        self.tx.send(ViewCmd::Eval(None, js)).unwrap();
    }

    fn new_request(&mut self) -> RequestBuilder {
        let id = {
            let id = self.next_request_id;
            self.next_request_id += 1;
            id
        };

        RequestBuilder::new(self.handle(), id)
    }

    /// Add new callback. Get descriptor of newly registered callback.
    fn add_callback(&mut self, f: Box<&'static Callback>) -> CallbackId {
        let id = self.next_callback_id;
        self.callbacks.insert(id, *f);
        self.next_callback_id += 1;
        id
    }

    /// Remove previously registered callback.
    ///
    /// # Panics
    /// This function will panic if callback is not present.
    fn remove_callback<'a, 'b>(&'a mut self, id: CallbackId) -> &'b Callback {
        self.callbacks.remove(&id).unwrap()
    }

    /// Find callback with given id.
    fn callback<'a, 'b>(&'a self, id: CallbackId) -> Option<Box<&'b Callback>> {
        if let Some(f) = self.callbacks.get(&id) {
            Some(Box::new(f.clone()))
        } else {
            None
        }
    }

    /// Function that handles events from JavaScript.
    fn handler(&mut self, arg: &str) -> web_view::WVResult {
        use InCmd::*;

        match serde_json::from_str(arg).unwrap() {
            Callback {
                descriptor,
                args,
            } => {
                if let Some(f) = self.callback(descriptor) {
                    let s = self.handle();
                    f(s, args);
                }
            },

            ExistenceTest {
                request,
                found,
            } => {
                self.respond(request, ResponseValue::Bool(found));
            },

            Attribute {
                request,
                value,
            } => {
                self.respond(request, ResponseValue::Str(value));
            },
        }

        Ok(())
    }

    /// Remove previously registered request by id if any. Function returns a sender
    /// that was to be used to wake up the waiting function.
    fn remove_request(&mut self, id: RequestId) -> Option<mpsc::Sender<ResponseValue>> {
        self.requests.remove(&id)
    }

    /// Save request response. Remove request from waiting list and wake up the waiter.
    fn respond(&mut self, id: RequestId, val: ResponseValue) {
        if let Some(r) = self.requests.remove(&id) {
            let _result = r.send(val);
            // TODO use result
        }
    }
}

impl ViewWrap {

    pub fn new_builder() -> ViewBuilder {
        View::new_builder()
    }

    pub fn new_from_builder(builder: ViewBuilder) -> ViewWrap {
        View::new_from_builder(builder)
    }

    /// Get new handle on this view.
    pub fn handle(&self) -> ViewHandle {
        let view = self.inner.read().unwrap();
        view.handle()
    }

    /// Get access to root component Arc.
    pub fn root_component(&mut self) -> ComponentHandle {
        let mut view = self.inner.write().unwrap();
        view.root_component()
    }

    /// Inject styles to the view.
    pub fn inject_css(&mut self, css: String) {
        let mut view = self.inner.write().unwrap();
        view.inject_css(css)
    }

    /// Run given JS code and wait for result.
    pub fn eval_wait(&mut self, js: String) -> WVResult {
        let mut view = self.inner.write().unwrap();
        view.eval_wait(js)
    }

    /// Run given JS code without waiting for result.
    pub fn eval(&mut self, js: String) {
        let mut view = self.inner.write().unwrap();
        view.eval(js)
    }
}

impl ViewBuilder {

    pub fn debug(mut self, debug: bool) -> Self {
        self.debug = debug;
        self
    }

    pub fn fullscreen(mut self, fullscreen: bool) -> Self {
        self.fullscreen = fullscreen;
        self
    }

    pub fn resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    pub fn size(mut self, width: usize, height: usize) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    pub fn title(mut self, title: String) -> Self {
        self.title = Some(title);
        self
    }

    pub fn build(self) -> ViewWrap {
        View::new_from_builder(self)
    }
}

impl RequestBuilder {

    fn new(view: ViewHandle, id: RequestId) -> Self {
        let (tx, rx) = mpsc::channel();
        RequestBuilder {
            view,
            id,
            tx,
            rx,
            js: None,
        }
    }

    pub fn id(&self) -> RequestId {
        self.id
    }

    /// Attach JavaScript code to be run.
    pub fn attach_js(mut self, js: String) -> Self {
        self.js = Some(js);
        self
    }

    /// Evaluate the request.
    pub fn eval(self) -> mpsc::Receiver<ResponseValue> {
        let js = self.js.unwrap();
        let id = self.id;
        let mut lock = self.view.write().unwrap();

        let err = {
            // Save the sender to the view so callback could send the value to listener.
            lock.requests.insert(id, self.tx);
            lock.eval_wait(js).is_err()
        };
        if err {
            // Evaluation failed so response will never arrive. Delete the entry.
            lock.remove_request(id);
        }

        self.rx
    }

    /// Attach this JavaScript code and evaluate it.
    pub fn run(self, js: String) -> mpsc::Receiver<ResponseValue> {
        self.attach_js(js).eval()
    }
}

impl Element for RootComponent {

    fn tag_name(&self) -> TagName {
        self.base.tag_name()
    }

    fn id(&self) -> &String {
        self.base.id()
    }

    fn view(&self) -> RwLockReadGuardRef<View> {
        self.base.view()
    }

    fn view_mut(&mut self) -> RwLockWriteGuardRefMut<View> {
        self.base.view_mut()
    }
}

impl Container for RootComponent {

    fn add_component(&mut self, component: Box<dyn Component>)
            -> Result<ComponentHandle, AddComponentError> {
        let html = component.generated_html();
        let id = self.name();

        let js = format!("\
            var i = document.getElementById('{}');
            i.innerHTML += '{}';
        ", id, html.to_string());

        let result = self.base.add_component(component);
        if let Err(e) = result {
            return Err(e);
        }
        self.view_mut().eval(js);
        Ok(result.unwrap())
    }

    fn remove_component(&mut self, component: &ComponentHandle) -> Option<()> {
        let result = self.base.remove_component(component);
        if let Some(_) = result {
            let js = format!("\
                var i = document.getElementById('{}');
                i.outerHTML = '';
            ", component.read().as_owner().name());
            self.view_mut().eval(js);
            Some(())
        } else {
            None
        }
    }

    fn has_component(&self, component: &ComponentHandle) -> bool {
        self.base.has_component(component)
    }
}

impl ChildrenLogic for RootComponent {
    // Root component only accepts components.

    fn add_child(&mut self, child: Box<Element>) -> Result<(), ChildrenLogicAddError> {
        Err(ChildrenLogicAddError::UnexpectedChild(child))
    }

    fn remove_child(&mut self, _child: &str) -> Option<Box<dyn Element>> {
        None
    }

    fn contains_child(&self, _child: &str) -> bool {
        false
    }
}

impl Component for RootComponent {

    fn generated_html(&self) -> &Node {
        self.base.generated_html()
    }

    fn elements(&self) -> &HashMap<String, Box<Element>> {
        self.base.elements()
    }

    fn element_by_origin(&self, id: &str) -> Option<&Box<Element>> {
        self.base.element_by_origin(id)
    }

    fn name(&self) -> &String {
        self.base.name()
    }

    fn self_element(&self) -> &Box<Element> {
        self.base.self_element()
    }

    fn components(&self) -> &HashSet<ComponentHandle> {
        self.base.components()
    }

    fn class(&self) -> &ClassHandle {
        self.base.class()
    }
}

/// Command that can be received from JavaScript front-end.
#[derive(Deserialize, Clone, Debug)]
#[serde(tag = "incmd", rename_all = "camelCase")]
enum InCmd {

    /// Some event triggered callback. Passed arguments are stored in JSON format.
    /// Descriptor of callback function is used to identify which registered callback should be
    /// called.
    Callback {
        descriptor: usize,
        args: String,
    },

    /// Response command for a test whether some element still exists.
    ExistenceTest {
        request: RequestId,
        found: bool,
    },

    /// Response to attribute value request.
    Attribute {
        request: RequestId,
        value: String,
    },
}

/// Value received from JavaScript front-end.
enum ResponseValue {
    Bool(bool),
    Str(String)
}
