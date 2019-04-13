use crate::{Interface, ResponseValue};
use std::fmt::Debug;
use htmldom_read::{Node};

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
                "document.getElementById({}).{} = {}", self.id(), name, value
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
        #[derive(Clone, Debug)]
        pub struct $name {
           interface: Interface,
           id: String,
        }

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
            TagName::A          => Box::new(A           { interface, id }),
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
