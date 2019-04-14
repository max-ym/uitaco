use crate::{Interface, ResponseValue, Callback};
use std::fmt::Debug;
use htmldom_read::{Node};
use crate::events::OnClick;
use std::any::{Any, TypeId};
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

#[derive(Clone, Debug)]
pub enum TagName {
    A,
    Img,
    P,
    Span,

    Unknown(String)
}

/// Element in the HTML DOM that can be accessed by Rust interface.
pub trait Element: Debug {

    /// Tag name of the element.
    fn tag_name(&self) -> TagName;

    /// HTML content of this element if it still exists.
    fn dom_html(&mut self) -> Option<String> {
        let req = self.interface_mut().new_request();
        let js = format!(r#"
            var inner = document.getElementById({}).outerHTML;
            window.external.invoke(JSON.stringify({{
                request: {},
                value: inner
            }}));
        "#, self.id(), req.id());
        let rx = req.run(js);
        let response = rx.recv().unwrap();
        if let ResponseValue::Str(s) = response {
            if s.is_empty() { // TODO possible output is 'undefined'. Check the case.
                None
            } else {
                Some(s)
            }
        } else {
            // Inner HTML request cannot return any other response type.
            unreachable!();
        }
    }

    /// Get attribute value of the element if any. Even if attribute is present but is empty
    /// None is returned.
    fn attribute(&self, name: &str) -> Option<String> {
        unimplemented!()
    }

    /// Set attribute with given name to given value.
    fn set_attribute(&mut self, name: &str, value: &str) {
        self.interface().eval(
            &format!(
                "document.getElementById('{}').setAttribute('{}', '{}');", self.id(), name, value
            )
        );
    }

    /// Element ID.
    fn id(&self) -> &String;

    /// Change element ID.
    fn set_id(&mut self, new_id: &str) {
        self.set_attribute("id", new_id)
    }

    fn interface(&self) -> &Interface;

    fn interface_mut(&mut self) -> &mut Interface {
        let ptr = self.interface() as *const Interface as *mut Interface;
        unsafe { &mut *(ptr) }
    }

    /// Check whether this element still exists.
    /// Actions on non-existing elements have no effect.
    fn exists(&mut self) -> bool {
        self.dom_html().is_some()
    }
}

/// Text content can be set to some text value and read this content back.
pub trait TextContent: Element {

    /// Get text contained by this element.
    fn text(&self) -> String {
        if let Some(s) = self.attribute("textContent") {
            s
        } else {
            String::new()
        }
    }

    fn set_text<T: AsRef<str>>(&mut self, text: T) {
        self.set_attribute("textContent", text.as_ref())
    }
}

macro_rules! elm_impl {
    ($name: ident) => {
        impl Element for $name {

            fn interface(&self) -> &Interface {
                &self.interface
            }

            fn id(&self) -> &String {
                &self.id
            }

            fn tag_name(&self) -> TagName {
                TagName::$name
            }
        }
    }
}

/// Wrap that gives access to the dynamic element which is known to be of given type.
#[derive(Debug)]
pub struct Wrap<T: Element> {
    element: Box<dyn Element>,
    _p: PhantomData<T>,
}

#[derive(Debug)]
pub struct A {
    interface: Interface,
    id: String,

    onclick: OnClick<A>,
}

#[derive(Clone, Debug)]
pub struct Img {
    interface: Interface,
    id: String,
}

#[derive(Clone, Debug)]
pub struct P {
    interface: Interface,
    id: String,
}

#[derive(Clone, Debug)]
pub struct Span {
    interface: Interface,
    id: String,
}

elm_impl!(A);
elm_impl!(Img);
elm_impl!(P);
elm_impl!(Span);

#[derive(Clone, Debug)]
pub struct Unknown {
    interface: Interface,
    id: String,
    name: String,
}

impl<T> Wrap<T> where T: Element {

    /// Wrap given element.
    ///
    /// # Safety
    /// Programmer must be sure this element has expected type.
    pub unsafe fn new(element: Box<dyn Element>) -> Self {
        Wrap {
            element,
            _p: Default::default(),
        }
    }
}

impl<T> Deref for Wrap<T> where T: Element {

    type Target = Box<T>;

    fn deref(&self) -> &Box<T> {
        let b = &self.element;
        let ptr = b as *const Box<dyn Element> as *const Box<T>;
        unsafe { &*ptr }
    }
}

impl<T> DerefMut for Wrap<T> where T: Element {

    fn deref_mut(&mut self) -> &mut Box<T> {
        let b = &mut self.element;
        let ptr = b as *mut Box<dyn Element> as *mut Box<T>;
        unsafe { &mut *ptr }
    }
}

impl From<&str> for TagName {

    fn from(s: &str) -> Self {
        use self::TagName::*;

        match s.to_lowercase().as_str() {
            "a"         => A,
            "img"       => Img,
            "p"         => P,
            "span"      => Span,

            _           => Unknown(String::from(s)),
        }
    }
}

impl TagName {

    /// Create implementation of the tag by it's tag name.
    pub fn new_impl(&self, interface: Interface, id: String) -> Box<dyn Element> {
        match self {
            TagName::A => {
                let mut b = Box::new(A {
                    interface,
                    id,
                    onclick: unsafe { OnClick::null() },
                });
                let onclick = unsafe { OnClick::new(&mut *b) };
                b.onclick = onclick;
                b
            },

            TagName::Img        => Box::new(Img         { interface, id }),
            TagName::P          => Box::new(P           { interface, id }),
            TagName::Span       => Box::new(Span        { interface, id }),

            TagName::Unknown(name) => Box::new(Unknown {
                interface,
                id,
                name: name.clone(),
            }),
        }
    }

    /// Try creating TagName from this node.
    pub fn try_from_node(node: &Node) -> Option<Self> {
        let tag_name = node.tag_name();
        if let Some(tag_name) = tag_name {
            let tag_name = TagName::from(tag_name);
            Some(tag_name)
        } else {
            None
        }
    }

    /// Try creating implementation of the Element from this node.
    ///
    /// # Failures
    /// Node must contain ID of the element. It also is required to contain opening tag
    /// which corresponds to element tag. If either of conditions is not met this function
    /// will return None.
    pub fn try_impl_from_node(node: &Node, interface: Interface) -> Option<Box<dyn Element>> {
        let tag_name = Self::try_from_node(node);
        if let Some(tag_name) = tag_name {
            let id = node.attribute_by_name("id");
            if let Some(id) = id {
                Some(tag_name.new_impl(interface, id.values_to_string()))
            } else {
                None
            }
        } else {
            None
        }
    }
}

impl A {

    pub fn href(&self) -> String {
        if let Some(s) = self.attribute("href") {
            s
        } else {
            String::new()
        }
    }

    pub fn set_href<T: AsRef<str>>(&mut self, href: T) {
        self.set_attribute("href", href.as_ref())
    }

    pub fn onclick(&self) -> &OnClick<A> {
        &self.onclick
    }

    pub fn onclick_mut(&mut self) -> &mut OnClick<A> {
        &mut self.onclick
    }
}

impl TextContent for A {}

impl TextContent for P {}

impl TextContent for Span {}

impl Element for Unknown {

    fn tag_name(&self) -> TagName {
        TagName::Unknown(self.id.clone())
    }

    fn id(&self) -> &String {
        &self.id
    }

    fn interface(&self) -> &Interface {
        &self.interface
    }
}
