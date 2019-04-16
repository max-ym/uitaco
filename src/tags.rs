use crate::{Interface, ResponseValue};
use std::fmt::Debug;
use htmldom_read::{Node};
use crate::events::OnClick;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use std::fmt::Formatter;
use std::sync::Arc;

/// The functions that allow to load images concurrently.
pub mod image_loader {
    use std::sync::Arc;
    use crate::tags::Image;
    use crate::tags::ImageFormat;
    use std::collections::LinkedList;

    /// Load all images from binary format from the iterator. This function is concurrent.
    /// It will create multiple threads to process images in parallel. Returned value contains
    /// handles to all images in the order they appeared in the iterator.
    pub fn load_all(iter: &mut Iterator<Item = (Vec<u8>, ImageFormat)>) -> Vec<Arc<Image>> {
        use std::sync::mpsc;
        use std::thread;

        // Start loading images async.
        let recvs = {
            let mut list = LinkedList::new();
            for (arr, format) in iter {
                let (tx, rx) = mpsc::channel();
                list.push_back(rx);

                thread::spawn(move || {
                    let img = Image::from_binary(arr, format);
                    tx.send(img).unwrap();
                });
            }
            list
        };

        // Collect results.
        let mut vec = Vec::with_capacity(recvs.len());
        for rx in recvs {
            let image = rx.recv().unwrap();
            let arc = Arc::new(image);

            vec.push(arc);
        }

        vec
    }

    /// Load one image into Arc.
    pub fn load(bin: Vec<u8>, format: ImageFormat) -> Arc<Image> {
        let img = Image::from_binary(bin, format);
        Arc::new(img)
    }
}

#[derive(Clone, Debug)]
pub enum TagName {
    A,
    Canvas,
    H4,
    H5,
    Img,
    Li,
    P,
    Span,

    Unknown(String)
}

/// Supported canvas image formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpg,
}

/// Element in the HTML DOM that can be accessed by Rust interface.
pub trait Element: Debug {

    /// Tag name of the element.
    fn tag_name(&self) -> TagName;

    /// HTML content of this element if it still exists.
    fn dom_html(&mut self) -> Option<String> {
        let req = self.interface_mut().new_request();
        let js = format!("\
            var inner = document.getElementById({}).outerHTML;\
            window.external.invoke(JSON.stringify({{\
                incmd: 'attribute',
                request: {},\
                value: inner\
            }}));
        ", self.id(), req.id());
        let rx = req.run(js);
        let response = rx.recv();
        if let Err(_) = response {
            return None; // likely because Null element was accessed.
        }
        let response = response.unwrap();

        if let ResponseValue::Str(s) = response {
            if s.is_empty() {
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
        let request = self.interface().new_request();
        let id = request.id();

        let js = format!("\
            var attr = document.getElementById('{}').getAttribute({});\
            attr = attr == null ? '' : attr;\
            window.external.invoke(JSON.stringify ({{\
                incmd: 'attribute',\
                request: {},\
                value: attr\
            }}));\
        ", self.id(), name, id);

        let receiver = request.run(js);
        let attr = receiver.recv().unwrap();
        if let ResponseValue::Str(s) = attr {
            if s == "" {
                None
            } else {
                Some(s)
            }
        } else {
            unreachable!()
        }
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

    fn add_class(&mut self, class: &str) {
        let attr = self.attribute("class");
        let mut attr = if let Some(s) = attr {
            s
        }  else {
            String::with_capacity(class.len())
        };

        attr.push_str(class);
        self.set_attribute("class", &attr);
    }

    fn remove_class(&mut self, class: &str) {
        let attr = self.attribute("class");
        if attr.is_none() {
            self.set_attribute("class", class);
            return;
        }
        let attr = attr.unwrap();
        let split = attr.split_whitespace();

        let mut new_str = String::with_capacity(attr.len());
        for val in split {
            if val != class {
                new_str.push_str(val);
            }
        }

        self.set_attribute("class", &new_str);
    }

    fn has_class(&self, class: &str) -> bool {
        let attr = self.attribute("class");
        if attr.is_none() {
            return false;
        }
        let attr = attr.unwrap();

        let split = attr.split_whitespace();
        for s in split {
            if s == class {
                return true;
            }
        }
        false
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

pub trait ImageContent: Element {

    /// Set image data to this element.
    fn set_image(&mut self, img: Arc<Image>);

    /// Get image data of this element.
    fn image(&self) -> Option<&Arc<Image>>;

    /// Remove any supplied image data.
    fn remove_image(&mut self) -> Option<Arc<Image>>;
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

/// Image data of canvas.
#[derive(Clone)]
pub struct Image {
    base64: String,
    format: ImageFormat,
}

#[derive(Debug)]
pub struct A {
    interface: Interface,
    id: String,

    onclick: OnClick<A>,
}

#[derive(Debug)]
pub struct Canvas {
    interface: Interface,
    id: String,
}

#[derive(Clone, Debug)]
pub struct H4 {
    interface: Interface,
    id: String,
}

#[derive(Clone, Debug)]
pub struct H5 {
    interface: Interface,
    id: String,
}

#[derive(Clone, Debug)]
pub struct Img {
    interface: Interface,
    id: String,

    data: Option<Arc<Image>>,
}

#[derive(Clone, Debug)]
pub struct Li {
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
elm_impl!(Canvas);
elm_impl!(H4);
elm_impl!(H5);
elm_impl!(Img);
elm_impl!(Li);
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

impl Debug for Image {

    fn fmt(&self, fmt: &mut Formatter) -> std::fmt::Result {
        write!(fmt, "Image {{ base64: [char; ")?;
        write!(fmt, "{}", self.base64.len())?;
        write!(fmt, "], format: ")?;
        write!(fmt, "{:?}", self.format)?;
        write!(fmt, " }}")?;
        Ok(())
    }
}

impl From<&str> for TagName {

    fn from(s: &str) -> Self {
        use self::TagName::*;

        match s.to_lowercase().as_str() {
            "a"         => A,
            "canvas"    => Canvas,
            "h4"        => H4,
            "h5"        => H5,
            "img"       => Img,
            "li"        => Li,
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

            TagName::Canvas => {
                Box::new(Canvas {
                    interface,
                    id,
                })
            },

            TagName::H4 => Box::new(
                H4 {
                    interface,
                    id,
                }
            ),

            TagName::H5 => Box::new(
                H4 {
                    interface,
                    id,
                }
            ),

            TagName::Img => Box::new(
                Img {
                    interface,
                    id,
                    data: None,
                }
            ),

            TagName::Li => Box::new (
                Li {
                    interface,
                    id,
                }
            ),

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

impl ImageFormat {

    pub fn to_string(&self) -> String {
        use ImageFormat::*;
        match self {
            Jpg => "jpg",
            Png => "png",
        }.to_string()
    }
}

impl Image {

    /// Encode given array of bytes in Base64 encoding.
    pub fn base64(bin: Vec<u8>) -> String {
        base64::encode(&bin)
    }

    /// Generate image struct from given array.
    pub fn from_binary(bin: Vec<u8>, format: ImageFormat) -> Image {
        Image {
            base64: Self::base64(bin),
            format,
        }
    }

    /// Convert this image to string that can be supplied to 'src' attribute of <img> tag.
    pub fn to_img_string(&self) -> String {
        format!("data:image/{};base64,{}", self.format.to_string(), self.base64)
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

impl ImageContent for Img {

    fn set_image(&mut self, img: Arc<Image>) {
        self.data = Some(img);
        self.set_attribute("src", &self.data.as_ref().unwrap().to_img_string());
    }

    fn image(&self) -> Option<&Arc<Image>> {
        self.data.as_ref()
    }

    fn remove_image(&mut self) -> Option<Arc<Image>> {
        let mut img: Option<Arc<Image>> = None;
        std::mem::swap(&mut img, &mut self.data);
        img
    }
}

impl TextContent for A {}

impl TextContent for H4 {}

impl TextContent for H5 {}

impl TextContent for Li {}

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
