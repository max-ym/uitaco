//! Graphical User Interface for Rust using HTML code to create visual components and to build
//! UI from them. Also, attachment of CSS and JS is possible. This library
//! also uses `webview` crate to display the content and control the page as components
//! get modified by the user actions or by Rust back-end.

#[macro_use]
extern crate typed_html;
extern crate web_view;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
pub extern crate htmldom_read;
extern crate owning_ref;

use serde_derive::{Deserialize};
use web_view::{WebView as _WebView, Content};
use std::sync::{Arc, RwLock, mpsc};
use std::collections::{HashMap, HashSet};
use std::thread;
use crate::component::{ComponentBase, Class, InstanceBuilder, ComponentHandle, ComponentId, Component, Container, AddComponentError, ChildrenLogic, ChildrenLogicAddError, ClassHandle};
use typed_html::dom::DOMTree;
use crate::tags::{Element, TagName};
use htmldom_read::Node;

/// Components allow to build user interface using repeated patterns with binding to elements.
/// This allows to speed up building of UI. Binding allows to easily access contents from Rust.
pub mod component;

/// Tags that are used to access corresponding information of the page.
pub mod tags;

/// Root component must be added first.
const ROOT_COMPONENT_ID: ComponentId = 0;

type UserData = Vec<(String, String)>;
type WebView<'a> = _WebView<'a, UserData>;
type Callback = Fn(&Interface, String) + Send + Sync;
type RequestId = usize;

#[derive(Clone, Debug)]
pub struct InterfaceBuilder {
    debug: bool,
    fullscreen: bool,
    resizable: bool,
    width: usize,
    height: usize,
    title: Option<String>,
}

/// Interface for `WebView` and `Uitaco`.
struct InterfaceInner {
    view: WebView<'static>,

    next_callback_id: usize,
    callbacks: HashMap<usize, &'static (dyn Fn(&Interface, String) + Send + Sync)>,

    next_request_id: RequestId,
    requests: HashMap<RequestId, mpsc::Sender<ResponseValue>>,

    next_component_id: ComponentId,
    components: HashMap<ComponentId, Arc<RwLock<Box<dyn Component>>>>,
}

/// Handle to the Interface to allow to access it from different threads.
/// Can be safely cloned to be shared - it will still point to the same interface.
#[derive(Clone, Debug)]
pub struct Interface {
    i: Arc<RwLock<InterfaceInner>>,
}

struct RequestBuilder {
    interface: Interface,
    id: RequestId,
    js: Option<String>,
    rx: mpsc::Receiver<ResponseValue>,
    tx: mpsc::Sender<ResponseValue>,
}

#[derive(Debug)]
pub struct RootComponent {
    base: ComponentBase,
}

impl std::fmt::Debug for InterfaceInner {

    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        #[derive(Debug)]
        struct InterfaceInnerDebug<'a> {
            view: &'a WebView<'static>,

            next_callback_id: usize,

            next_request_id: RequestId,
            requests: &'a HashMap<RequestId, mpsc::Sender<ResponseValue>>,

            next_component_id: ComponentId,
            components: &'a HashMap<ComponentId, Arc<RwLock<Box<dyn Component>>>>,
        }

        let for_debug = InterfaceInnerDebug {
            view:               &self.view,
            next_callback_id:   self.next_callback_id,
            next_request_id:    self.next_request_id,
            requests:           &self.requests,
            next_component_id:  self.next_component_id,
            components:         &self.components,
        };

        write!(f, "{:?}", for_debug)
    }
}

impl Default for InterfaceBuilder {

    fn default() -> Self {
        InterfaceBuilder {
            debug: true,
            fullscreen: false,
            resizable: true,
            width: 640,
            height: 480,
            title: None,
        }
    }
}

impl InterfaceBuilder {

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

    pub fn build(self) -> Interface {
        Interface::new_from_builder(self)
    }
}

impl Interface {

    /// Create new builder to help building interface.
    pub fn new_builder() -> InterfaceBuilder {
         InterfaceBuilder::default()
    }

    /// Create new interface for WebView.
    pub fn new_from_builder(builder: InterfaceBuilder) -> Interface {
        let mut my_builder = web_view::builder();
        my_builder.debug = builder.debug;
        my_builder.resizable = builder.resizable;
        my_builder.title = "Unnamed Uitaco";
        my_builder.width = builder.width as _;
        my_builder.height = builder.height as _;

        // ID that will be assigned to element 'body' of the HTML document.
        let uitaco_body_id = "uitacoBody";

        let content = {
            let uitaco_body_id = typed_html::types::Id::new(uitaco_body_id);
            let i: DOMTree<String> = html!(
                <html>
                <head>
                    <title>"WebView + Uitaco"</title>
                </head>
                <body class=component::COMPONENT_MARK id=uitaco_body_id></body>
                </html>
            );
            i.to_string()
        };

        // Later used to initialize root component.
        let mut classes = Class::all_from_html(&content);
        let root_component_class = classes.remove(uitaco_body_id).unwrap();

        my_builder.content = Some(Content::Html(content));

        let inner = InterfaceInner {
            view: unsafe { std::mem::uninitialized() },

            next_callback_id: 0,
            callbacks: HashMap::new(),

            next_request_id: 0,
            requests: HashMap::new(),

            next_component_id: 0,
            components: HashMap::new(),
        };

        let mut interface = Interface {
            i: Arc::new(RwLock::new(inner)),
        };

        let mut iclone = interface.clone();
        let mut view = my_builder
            .invoke_handler(move |_view, arg| {
                iclone.handler(arg)
            })
            .user_data(UserData::new())
            .build().unwrap();

        // Initialize view and forget uninitialized value.
        {
            use std::mem::{swap, forget};
            swap(&mut interface.i.write().unwrap().view, &mut view);
            forget(view);
        }

        // Initialize root component.
        let root_component = {
            let mut builder = InstanceBuilder::new_for_class(root_component_class);

            builder.element_by_id_mut(uitaco_body_id).unwrap()
                .set_name(uitaco_body_id.to_string());
            let base = builder.build(interface.clone());

            RootComponent { base }
        };
        interface.add_component(Box::new(root_component));

        interface
    }

    /// Add new callback. Get descriptor of newly registered callback.
    fn add_callback<F>(&mut self, f: &'static F) -> usize
        where
            F: Fn(&Interface, String) + Send + Sync {
        let mut interface = self.i.write().unwrap();
        let id = interface.next_callback_id;
        interface.callbacks.insert(id, f);
        interface.next_callback_id += 1;
        id
    }

    /// Remove previously registered callback.
    fn remove_callback(&mut self, id: usize) -> &Callback {
        let mut interface = self.i.write().unwrap();
        interface.callbacks.remove(&id).unwrap()
    }

    /// Find callback with given id.
    fn callback(&self, id: usize) -> Option<Box<Callback>> {
        let interface = self.i.read().unwrap();
        if let Some(f) = interface.callbacks.get(&id) {
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
                    let s = self.clone();
                    f(&s, args);
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

    /// Run given JS code.
    pub fn eval(&self, js: &str) {
        self.i.write().unwrap().view.eval(js).unwrap();
    }

    fn new_request(&mut self) -> RequestBuilder {
        let id = {
            let mut write = self.i.write().unwrap();
            let id = write.next_request_id;
            write.next_request_id += 1;
            id
        };

        RequestBuilder::new(self.clone(), id)
    }

    /// Remove previously registered request by id if any. Function returns a sender
    /// that was to be used to wake up the waiting function.
    fn remove_request(&mut self, id: RequestId) -> Option<mpsc::Sender<ResponseValue>> {
        let mut i = self.i.write().unwrap();
        i.requests.remove(&id)
    }

    /// Save request response. Remove request from waiting list and wake up the waiter.
    fn respond(&mut self, id: RequestId, val: ResponseValue) {
        let mut i = self.i.write().unwrap();
        if let Some(r) = i.requests.get(&id) {
            let _result = r.send(val);
            i.requests.remove(&id);
        }
    }

    /// Get access to root component Arc.
    pub fn root_component(&mut self) -> ComponentHandle {
        ComponentHandle::new(self.clone(), ROOT_COMPONENT_ID)
    }

    /// Run execution loop until WebView exits. This function creates parallel thread that
    /// dispatches all events.
    pub fn run(&self) {
        let interface = self.clone();
        loop {
            match interface.i.write().unwrap().view.step() {
                Some(Ok(_)) => (),
                Some(_) => break,
                None => break,
            }
        }
    }

    /// Add new component to the interface and get a handle to it.
    fn add_component(&mut self, component: Box<dyn Component>) -> ComponentHandle {
        let id = {
            let mut this = self.i.write().unwrap();

            let id = this.next_component_id;
            this.next_component_id += 1;

            this.components.insert(id, Arc::new(RwLock::new(component)));
            id
        };

        ComponentHandle::new(self.clone(), id)
    }

    /// Try removing component from the interface. If it does not exist None is returned.
    /// Also, it may still be in use though it will still be removed from the interface
    /// and all new changes to the component will be therefore ignored.
    fn remove_component(&mut self, handle: &ComponentHandle) -> Option<()> {
        let mut this = self.i.write().unwrap();
        let option = this.components.remove(&handle.id());
        if let Some(_) = option {
            Some(())
        } else {
            None
        }
    }

    /// Inject styles to the view.
    pub fn inject_css(&mut self, css: &str) {
        self.i.write().unwrap().view.inject_css(css).unwrap();
    }
}

unsafe impl Send for Interface {}
unsafe impl Sync for Interface {}

impl PartialEq for Interface {

    fn eq(&self, other: &Interface) -> bool {
        Arc::ptr_eq(&self.i, &other.i)
    }
}

impl RequestBuilder {

    fn new(interface: Interface, id: RequestId) -> Self {
        let (tx, rx) = mpsc::channel();
        RequestBuilder {
            interface,
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
        let mut interface = self.interface;
        let js = self.js.unwrap();
        let id = self.id;
        thread::spawn(move || {
            let err = interface.i.write().unwrap().view.eval(&js).is_err();
            if err {
                // Evaluation failed so response will never arrive. Delete the entry.
                interface.remove_request(id);
            }
        });
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

    fn interface(&self) -> &Interface {
        self.base.interface()
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
        self.interface().eval(&js);
        Ok(result.unwrap())
    }

    fn remove_component(&mut self, component: &ComponentHandle) -> Option<()> {
        let result = self.base.remove_component(component);
        if let Some(_) = result {
            let js = format!("\
                var i = document.getElementById('{}');
                i.outerHTML = '';
            ", component.read().as_owner().name());
            self.interface().eval(&js);
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

    fn remove_child(&mut self, child: &str) -> Option<()> {
        None
    }

    fn contains_child(&self, child: &str) -> bool {
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
    Str(String),
    Empty,
}
