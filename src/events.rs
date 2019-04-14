use crate::{Callback, CallbackId, Interface};
use crate::tags::Element;
use std::ops::{Deref, DerefMut};

/// Event that can be generated by the item when user takes some action.
pub trait Event {

    /// The function that will be called when event appears.
    fn callback(&self) -> Option<Box<&Callback>>;

    /// Set new callback function.
    fn set_callback(&mut self, callback: Box<&'static Callback>);

    /// Remove any callback for this event.
    fn remove_callback(&mut self) -> Option<&Callback>;

    /// Check whether this event has set callback.
    fn is_set(&self) -> bool;
}

/// Wrapped for raw pointer that point to the parent element that is known to outlive current struct.
#[derive(Debug)]
struct Ref<E: Element> {
    parent: *mut E,
}

#[derive(Debug)]
pub struct OnClick<E: Element> {
    callback_id: Option<CallbackId>,
    elem: Ref<E>,
}

impl<E> Deref for Ref<E>
        where E: Element {

    type Target = E;

    fn deref(&self) -> &E {
        unsafe { &*self.parent }
    }
}

impl<E> DerefMut for Ref<E>
        where E: Element {

    fn deref_mut(&mut self) -> &mut E {
        unsafe { &mut *self.parent }
    }
}

impl<E> Ref<E> where E: Element {

    pub fn null() -> Self {
        Ref {
            parent: 0usize as _,
        }
    }
}

fn default_callback_fn(id: CallbackId) -> String {
    format!("
        window.external.invoke(JSON.stringify ({{
            descriptor: {},
            args: ''
        }}))
    ", id)
}

impl<E> Event for OnClick<E>
        where E: Element {

    fn callback(&self) -> Option<Box<&Callback>> {
        if let Some(id) = self.callback_id {
            Some(self.elem.interface().callback(id).unwrap())
        } else {
            None
        }
    }

    fn set_callback(&mut self, callback: Box<&'static Callback>) {
        if self.is_set() {
            self.remove_callback();
        }

        let id = self.elem.interface_mut().add_callback(callback);
        self.elem.set_attribute("onclick", &default_callback_fn(id));

        self.callback_id = Some(id);
    }

    fn remove_callback(&mut self) -> Option<&Callback> {
        if !self.is_set() {
            return None;
        }

        self.elem.set_attribute("onclick", "");
        Some(self.elem.interface_mut().remove_callback(self.callback_id.unwrap()))
    }

    fn is_set(&self) -> bool {
        self.callback_id.is_some()
    }
}

impl<E> OnClick<E>
        where E: Element {

    /// Create new OnClick event for given element. This function does not assign newly created
    /// event to the element but this event expects to be assigned just to that element.
    ///
    /// # Safety
    /// User should manually assign this event to its parent element.
    /// Otherwise, undefined behaviour.
    pub unsafe fn new(element: &mut E) -> Self {
        OnClick {
            callback_id: None,
            elem: Ref { parent: element as _ }
        }
    }

    /// Create new OnClick with null parent.
    ///
    /// # Safety
    /// No functions should be called before parent gets assigned. Otherwise it possibly will
    /// lead to null pointer access.
    pub unsafe fn null() -> Self {
        OnClick {
            callback_id: None,
            elem: Ref::null(),
        }
    }
}
