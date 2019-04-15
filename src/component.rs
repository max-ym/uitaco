use std::collections::{HashMap, HashSet, LinkedList};
use crate::tags::{Element, TagName};
use crate::{Interface};
use std::sync::{Arc, RwLock};
use std::fmt::Debug;
use htmldom_read::{Node, NodeAccess, Attribute, Children};
use owning_ref::{RwLockReadGuardRef, RwLockWriteGuardRefMut};
use std::hash::{Hasher, Hash};
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use rsgen::{OutputCharsType, gen_random_string};

/// This value must be stored in class attribute of tag which starts a component class.
pub const COMPONENT_MARK: &'static str = "uitacoComponent";

/// Skip this element and it's children when parsing file for components.
/// his also removes it's HTML code from all nodes of loaded classes.
pub const SKIP_ELEMENT_MARK: &'static str = "uitacoSkip";

pub type ClassHandle = Arc<Class>;
pub type ComponentId = usize;

/// Error that occur while adding new component to container.
#[derive(Debug)]
pub enum AddComponentError {

    /// Component has a class that is not supported by this container.
    UnexpectedClass,

    /// User-defined error.
    Custom(Box<dyn CustomAddComponentError>),
}

pub trait CustomAddComponentError: Debug {}

/// If you want to implement custom error use this trait and "Custom" variant of
/// `ChildrenLogicError`.
pub trait ChildrenLogicAddCustomError: Debug {}

/// Error occurred when tried to add a child to a component.
#[derive(Debug)]
pub enum ChildrenLogicAddError {

    /// Component does not know how to add a child of this class.
    UnexpectedChild(Box<dyn Element>),

    /// Cannot add one more child of this class.
    Overflow(Box<dyn Element>),

    /// Child with same ID is already present.
    AlreadyPresent,

    /// User-defined error type.
    Custom(Box<dyn ChildrenLogicAddCustomError>, Box<dyn Element>),
}

/// Logic of how to add a children component to a parent component.
pub trait ChildrenLogic {

    /// Try adding a child to a component. It can fail if component cannot accept given child.
    /// Otherwise the reference to added child is returned.
    fn add_child(&mut self, child: Box<dyn Element>)
        -> Result<(), ChildrenLogicAddError>;

    /// Try removing a child from this component. It can fail if child is not present.
    fn remove_child(&mut self, child: &str) -> Option<Box<dyn Element>>;

    /// Check whether this component contains given child.
    fn contains_child(&self, child: &str) -> bool;
}

/// Thing that can contain other components.
pub trait Container: Element {

    /// Add new component to the container. On success a reference to this component is returned.
    fn add_component(&mut self, component: Box<dyn Component>)
        -> Result<ComponentHandle, AddComponentError>;

    /// Remove given component from the container if it is located in it.
    fn remove_component(&mut self, component: &ComponentHandle) -> Option<()>;

    /// Check whether this component is stored in this container.
    fn has_component(&self, component: &ComponentHandle) -> bool;
}

/// Component that is an element that has advanced logic and know how to drive attached
/// elements and other components.
pub trait Component: Element + Container + ChildrenLogic {

    /// HTML code of the component after build.
    fn generated_html(&self) -> &Node;

    /// All accessible elements of this component.
    fn elements(&self) -> &HashMap<String, Box<dyn Element>>;

    /// Get element by original ID.
    fn element_by_origin(&self, id: &str) -> Option<&Box<dyn Element>>;

    /// Actual assigned name for this component. This is a new value, not the original as it was
    /// in the source HTML but as it stands in generated HTML.
    fn name(&self) -> &String;

    /// Element that holds the whole component.
    fn self_element(&self) -> &Box<dyn Element>;

    /// All sub-components of this component.
    fn components(&self) -> &HashSet<ComponentHandle>;

    /// Component class from which it was instantiated.
    fn class(&self) -> &ClassHandle;

    /// Check whether this component is of given class.
    fn is_of_class(&self, class: &ClassHandle) -> bool {
        let this = self.class();
        Arc::ptr_eq(this, class)
    }
}

/// Perform more advanced component initialization.
/// Typical component initialization stages were divided into functions that are run in
/// particular order after newly created component reaches required state.
pub trait Initialize {

    /// The first stage of initialization.
    ///
    /// Assign placeholders unique IDs so that later it was possible to use them as elements.
    fn initialize_placeholders(&mut self);

    /// The second stage of initialization.
    ///
    /// Create a base component from a builder. Load it to the interface and wrap the raw base into
    /// specific component structure.
    fn initialize_base(&mut self);

    /// The third stage of initialization.
    ///
    /// Load all the elements and wrap them to make
    /// easily accessible and allow to modify state of component parts.
    fn initialize_elements(&mut self);

    /// The fourth stage of initialization.
    ///
    /// Load all sub-components and initialize them.
    fn initialize_components(&mut self);

    /// Perform all initialization stages.
    fn initialize(&mut self) {
        self.initialize_placeholders();
        self.initialize_base();
        self.initialize_elements();
        self.initialize_components();
    }
}

/// Struct that points to a place of HTML string where ID can be inserted.
/// It is created for elements that initially have IDs in a component code.
#[derive(Clone, Debug)]
pub struct Placeholder {

    /// Initial name. Node id for which this placeholder was created.
    initial: String,

    /// New ID if there was any created.
    new: Option<String>,
}

/// Component class that can be instantiated to be added to the HTML DOM.
#[derive(Debug)]
pub struct Class {

    /// The name of the component class.
    name: String,

    /// Full HTML code of this component. All IDs are removed.
    html: Arc<Node>,

    placeholders: HashMap<String, Placeholder>,
}

/// Builder to instantiate component.
#[derive(Debug, Clone)]
pub struct InstanceBuilder {

    /// Original class.
    class: ClassHandle,

    /// Placeholders for this instance.
    placeholders: HashMap<String, Placeholder>,
}

/// Built component that is ready to be added to a page.
#[derive(Debug)]
pub struct ComponentBase {
    /// Interface to which this component was bound.
    interface: Interface,

    /// Class from which component was instantiated.
    class: ClassHandle,

    /// Final HTML code with modified IDs and other things performed over original class code.
    /// Can be changed during execution when new elements get added or removed or other
    /// events of the component occur.
    html: Node,

    /// Rust-accessible front-end bindings to elements of this component.
    /// There is also a component-element which contains the whole component.
    /// The key is an original name of the element (not actual in generated HTML code).
    elements: HashMap<String, Box<dyn Element>>,

    /// Components that were added to this component.
    components: HashSet<ComponentHandle>,
}

/// Handle to a component registered in the interface.
#[derive(Clone, Debug)]
pub struct ComponentHandle {
    interface: Interface,
    id: usize,
    lock: Arc<RwLock<Box<dyn Component>>>,
}

/// Handle to a component with a particular type.
pub struct ComponentHandleT<T: Component> {
    handle: ComponentHandle,
    _phantom: PhantomData<T>,
}

impl Placeholder {

    /// Try creating new Placeholder for given node.
    ///
    /// # Failure
    /// If node has no ID attribute then placeholder cannot be created.
    fn new(node: &Node) -> Option<Self> {
        let id = node.attribute_by_name("id");
        if let Some(id) = id {
            Some(
                Placeholder {
                    initial: id.first_value().to_owned(),
                    new: None,
                }
            )
        } else {
            None
        }
    }

    /// Set new ID in this placeholder.
    pub fn set_name(&mut self, new_name: String) {
        self.new = Some(new_name);
    }

    /// Remove any previously set ID.
    pub fn remove_name(&mut self) {
        self.new = None;
    }

    /// Get newly set ID of this placeholder.
    pub fn name(&self) -> Option<&String> {
        if let Some(t) = &self.new {
            Some(t)
        } else {
            None
        }
    }

    /// Initial ID of this placeholder.
    pub fn initial(&self) -> &String {
        &self.initial
    }

    /// Generate random name. Can be used when no exact name is necessary and it is enough that
    /// this element just exists and is accessible by any name.
    /// This is likely the way you would want to generate names.
    pub fn generate_name(&mut self) -> &String {
        let len = 15;
        let prefix = "autogen";
        let mut s = String::with_capacity(15);
        s.push_str(prefix);
        let len = len - prefix.len();

        // Generate random string.
        let oct = OutputCharsType::LatinAlphabetAndNumeric {
            use_lower_case: true,
            use_upper_case: true
        };
        let random = gen_random_string(len, oct);
        s.push_str(&random);

        // Save name.
        self.set_name(s);

        self.name().unwrap()
    }
}

impl Class {

    /// Create class from given node that was read from HTML code corresponding for this class.
    /// The first node must be the component class tag. Otherwise None is returned.
    /// This function does not expect to receive root node, instead it expects the leaf
    /// with the data for component.
    fn try_one_from_node(node: Arc<Node>) -> Option<Self> {
        if node.is_root() {
            // This node is a root of the tree.
            return None;
        }

        // Remove the nodes which want to be skipped.
        let node = {
            // Wrap node into root.
            let node = NodeAccess::wrap_to_root(node.into()).unwrap();

            // Function to check whether this children list has nodes to skip.
            fn has_skips(children: &Children) -> bool {
                for child in children.iter() {
                    // Check child.
                    let attr = child.attribute_by_name("class");
                    let skip = if let Some(attr) = attr {
                        attr.values().contains(&SKIP_ELEMENT_MARK.to_string())
                    } else {
                        false
                    };

                    if skip {
                        return true;
                    }

                    // Check children of a child.
                    let skip = has_skips(child.children());
                    if skip {
                        return true;
                    }
                }
                false
            }

            // Check whether some nodes must be skipped.
            let needs_skips = has_skips(node.children());

            // Edit nodes if needed and get sharable node tree.
            if needs_skips {
                // Owned access is required to edit the node's content.
                let mut node = node.to_owned();

                // Remove children that need to be skipped.
                fn clean(children: &mut Children) {
                    let mut skiplist = LinkedList::new();
                    let mut i = 0;
                    while i < children.len() {
                        let child = children.get(i).unwrap();

                        if let Some(attr) = child.attribute_by_name("class") {
                            let skip
                                = attr.values().contains(&SKIP_ELEMENT_MARK.to_string());
                            if skip {
                                skiplist.push_back(i);
                            }
                        }

                        i += 1;
                    }

                    // Remove children that were marked for removal.
                    let mut iter = skiplist.iter();
                    while let Some(i) = iter.next_back() {
                        children.remove(*i);
                    }

                    // Iterate through all children left to find any removal marks in them.
                    for child in children.iter_mut() {
                        clean(child.try_mut().unwrap().children_mut());
                    }
                }
                clean(node.children_mut());

                NodeAccess::Owned(node).to_sharable()
            } else {
                node.to_sharable()
            }
        };

        // Node must contain component mark and class name.
        let name = {
            // Get node that contains component tag.
            let node = node.children().iter().next().unwrap();

            let class_attr = node.attribute_by_name("class");
            if let Some(class_attr) = class_attr {
                if !class_attr.values().contains(&COMPONENT_MARK.to_string()) {
                    return None;
                }
            } else {
                return None;
            }

            let attr = node.attribute_by_name("id");
            if let Some(name) = attr {
                name.first_value()
            } else {
                return None;
            }
        };

        // Create placeholders for parent and children.
        let placeholders = {
            // Add children.
            let fetch = node.children_fetch()
                .key("id");
            let result = fetch.fetch();
            let mut map
                = HashMap::with_capacity(result.len() + 1); // +1 for root placeholder.
            for node in result {
                let p = Placeholder::new(node).unwrap();
                map.insert(p.initial().to_owned(), p);
            }

            // Add parent.
            map.insert(name.to_owned(), Placeholder {
                initial: name.to_owned(),
                new: None,
            });

            map.shrink_to_fit();
            map
        };

        Some(Class {
            name: name.to_owned(),
            html: node,
            placeholders
        })
    }

    /// Try loading component class from HTML. First found component class tag will be used as
    /// a component class.
    pub fn try_from_html(html: &str) -> Option<Self> {
        let node = Node::from_html(html, &Default::default());
        if let Err(_) = node {
            return None;
        }
        if let None = node.as_ref().unwrap() {
            return None;
        }
        let node = node.unwrap().unwrap();

        // Unwrap root node and get first child which is expected to contain class.
        // Then get each next children until component is created or no more children
        // is left.
        for child in node.children().iter() {
            let node = child.to_sharable();
            if let Some(class) = Class::try_one_from_node(node) {
                return Some(class);
            }
        }
        None
    }

    /// Load all classes from this HTML.
    pub fn all_from_html(html: &str) -> HashMap<String, Class> {
        // Get node that presents given document.
        let node = Node::from_html(html, &Default::default());
        if let Err(_) = node {
            return Default::default();
        } else if let Ok(ok) = &node {
            if let None = ok {
                return Default::default();
            }
        };
        let node = node.unwrap().unwrap();

        // Find nodes that contains component.
        let component_nodes = node.children_fetch()
            .value_part(COMPONENT_MARK)
            .key("class")
            .fetch();

        let mut map = HashMap::new();
        for node in component_nodes {
            let node = node.to_sharable();
            let class = Class::try_one_from_node(node).unwrap();
            map.insert(class.name.clone(), class);
        }
        map
    }

    /// Get name (id) of this class.
    pub fn name(&self) -> &String {
        &self.name
    }

    /// All placeholders of a class.
    pub fn placeholders(&self) -> &HashMap<String, Placeholder> {
        &self.placeholders
    }

    /// Create class handle from this owned class.
    pub fn into_handle(self) -> ClassHandle {
        Arc::new(self)
    }
}

impl InstanceBuilder {

    /// Create class instance builder for given class.
    pub fn new_for_class(class: Class) -> Self {
        let handle = Arc::new(class);
        Self::new_for_handle(handle)
    }

    /// Create class instance builder for given class by the handle.
    pub fn new_for_handle(handle: ClassHandle) -> Self {
        let placeholders = handle.placeholders.to_owned();

        InstanceBuilder {
            class: handle,
            placeholders,
        }
    }

    /// Get element by HTML ID.
    pub fn element_by_id_mut(&mut self, id: &str) -> Option<&mut Placeholder> {
        self.placeholders.get_mut(id)
    }

    /// Build the component for given interface.
    pub fn build(self, interface: Interface) -> ComponentBase {
        let class = self.class;

        let mut html = {
            let mut html = class.html.as_ref().to_owned();
            let owned = html.children().to_all_owned();
            *html.children_mut() = owned;
            html
        };

        let elements = {
            let mut elements
                = HashMap::with_capacity(class.placeholders.len());
            for ph in self.placeholders {
                // Get node.
                let initial = ph.0;
                let ph = ph.1;

                let mut fetch = html.children_fetch_mut()
                    .key("id")
                    .value(&initial)
                    .fetch_mut();
                let ph_node = fetch.iter_mut().next().unwrap();

                if let NodeAccess::Owned(ref mut node) = ph_node {
                    // Reset ID of the node (element) with the ID in the placeholder.
                    let new_id = if let Some(id) = ph.new {
                        id
                    } else {
                        "".to_string()
                    };
                    let attr = Attribute::from_name_and_values(
                        "id".to_string(), vec![new_id]
                    ).unwrap();
                    node.overwrite_attribute(attr);

                    // Register element.
                    let elem = TagName::try_impl_from_node(
                        node, interface.clone()
                    ).unwrap();
                    elements.insert(initial.to_owned(), elem);
                } else {
                    unreachable!()
                };
            }
            elements
        };

        ComponentBase {
            interface,
            class,
            html,
            elements,
            components: Default::default(),
        }
    }
}

impl Element for ComponentBase {

    fn tag_name(&self) -> TagName {
        self.self_element().tag_name()
    }

    fn id(&self) -> &String {
        &self.name()
    }

    fn interface(&self) -> &Interface {
        &self.interface
    }
}

impl Container for ComponentBase {
    // Implementation does not change HTML as it is expected that this component struct
    // is used as a base for bigger more specific component which knows how to change HTML.
    // It will use this functions to keep track of components.

    fn add_component(&mut self, component: Box<dyn Component>)
            -> Result<ComponentHandle, AddComponentError> {
        let handle = self.interface.add_component(component);
        self.components.insert(handle.clone());
        Ok(handle)
    }

    fn remove_component(&mut self, component: &ComponentHandle) -> Option<()> {
        let found = self.components.remove(&component);
        if found {
            self.interface.remove_component(component);
            Some(())
        } else {
            None
        }
    }

    fn has_component(&self, component: &ComponentHandle) -> bool {
        self.components.contains(component)
    }
}

impl ChildrenLogic for ComponentBase {
    // Implementation does not change HTML as it is expected that this component struct
    // is used as a base for bigger more specific component which knows how to change HTML.
    // It will use this functions to keep track of components.

    fn add_child(&mut self, child: Box<Element>)
            -> Result<(), ChildrenLogicAddError> {
        let id = child.id();
        if self.elements.contains_key(id) {
            Err(ChildrenLogicAddError::AlreadyPresent)
        } else {
            self.elements.insert(id.to_owned(), child);
            Ok(())
        }
    }

    fn remove_child(&mut self, child: &str) -> Option<Box<dyn Element>> {
        let option = self.elements.remove(child);
        if let Some(e) = option {
            Some(e)
        } else {
            None
        }
    }

    fn contains_child(&self, child: &str) -> bool {
        self.elements.contains_key(child)
    }
}

impl Component for ComponentBase {

    fn generated_html(&self) -> &Node {
        &self.html
    }

    fn elements(&self) -> &HashMap<String, Box<dyn Element>> {
        &self.elements
    }

    fn element_by_origin(&self, id: &str) -> Option<&Box<dyn Element>> {
        self.elements.get(id)
    }

    fn name(&self) -> &String {
        self.html.children().get(0).unwrap() // Unwrap root node.
            .attribute_by_name("id").unwrap().first_value()
    }

    fn self_element(&self) -> &Box<dyn Element> {
        self.elements.get(self.class.name()).unwrap()
    }

    fn components(&self) -> &HashSet<ComponentHandle> {
        &self.components
    }

    fn class(&self) -> &ClassHandle {
        &self.class
    }
}

impl ComponentBase {

    /// Current mutable HTML code of the component.
    pub fn current_html_mut(&mut self) -> &mut Node {
        &mut self.html
    }

    /// Get element by original ID.
    pub fn element_by_origin_mut(&mut self, id: &str) -> Option<&mut Box<dyn Element>> {
        self.elements.get_mut(id)
    }

    /// All accessible elements of this component.
    pub fn elements_mut(&mut self) -> &mut HashMap<String, Box<dyn Element>> {
        &mut self.elements
    }
}

impl ComponentHandle {

    /// Create new component handle for given component (by id) in the interface.
    pub fn new(interface: Interface, id: ComponentId) -> Self {
        let lock
            = interface.i.read().unwrap()
            .components.get(&id).unwrap()
            .clone();

        ComponentHandle {
            interface,
            id,
            lock
        }
    }

    pub fn id(&self) -> ComponentId {
        self.id
    }

    pub fn read(&self) -> RwLockReadGuardRef<Box<dyn Component>> {
        RwLockReadGuardRef::new(self.lock.read().unwrap())
    }

    pub fn write(&mut self) -> RwLockWriteGuardRefMut<Box<dyn Component>> {
        RwLockWriteGuardRefMut::new(self.lock.write().unwrap())
    }
}

impl Hash for ComponentHandle {

    fn hash<T: Hasher>(&self, hasher: &mut T) {
        self.id.hash(hasher);
    }
}

impl PartialEq for ComponentHandle {

    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for ComponentHandle {}

impl<T> ComponentHandleT<T>
    where T: Component
{

    /// Create new handle with type for given raw handle. This is up to programmer
    /// to ensure the type of passed component is correct.
    pub unsafe fn new(handle: ComponentHandle) -> Self {
        ComponentHandleT {
            handle,
            _phantom: Default::default()
        }
    }
}

impl<T> Deref for ComponentHandleT<T>
    where T: Component {

    type Target = Arc<RwLock<Box<T>>>;

    fn deref(&self) -> &Self::Target {
        let lock = &self.handle.lock;
        let ptr = lock as *const Arc<RwLock<Box<dyn Component>>>;
        let ptr = ptr as *const Self::Target;
        unsafe { &*ptr }
    }
}

impl<T> DerefMut for ComponentHandleT<T>
    where T: Component {

    fn deref_mut(&mut self) -> &mut Self::Target {
        let lock = &mut self.handle.lock;
        let ptr = lock as *mut Arc<RwLock<Box<dyn Component>>>;
        let ptr = ptr as *mut Self::Target;
        unsafe { &mut *ptr }
    }
}

#[cfg(test)]
mod tests {
    use typed_html::dom::DOMTree;
    use crate::component::{Class};
    use crate::component::{COMPONENT_MARK, SKIP_ELEMENT_MARK};
    use typed_html::types::Id;

    #[test]
    fn class_from_html() {
        let name = "mydiv";
        let id = Id::new(name);
        let html = {
            let dom: DOMTree<String> = html!(
                <div class=COMPONENT_MARK id=id>
                    <p>"Some text"</p>
                </div>
            );
            dom.to_string()
        };

        let class = Class::try_from_html(&html).unwrap();
        assert_eq!(class.name(), name);
    }

    #[test]
    fn all_classes_from_html() {
        let html: DOMTree<String> = html!(
            <body>
                <p class=COMPONENT_MARK id="comp1"></p>
                <div class=COMPONENT_MARK id="comp2">
                    <p id="pl">"Some text"</p>
                </div>
            </body>
        );
        let html = html.to_string();

        let map = Class::all_from_html(&html);
        assert!(map.contains_key("comp1"));
        assert!(map.contains_key("comp2"));

        // Check whether there are a placeholders for all components and elements.
        let comp1 = map.get("comp1").unwrap();
        let comp2 = map.get("comp2").unwrap();
        comp1.placeholders().get("comp1").unwrap();
        comp2.placeholders().get("comp2").unwrap();
        comp2.placeholders().get("pl").unwrap();
        assert!(comp2.placeholders().get("comp1").is_none());
    }

    #[test]
    fn class_from_html_skip() {
        let html = {
            let dom: DOMTree<String> = html!(
                <div class=COMPONENT_MARK id="some">
                    <p class=SKIP_ELEMENT_MARK id="other">"Some text"</p>
                </div>
            );
            dom.to_string()
        };

        let map = Class::all_from_html(&html);
        assert!(map.contains_key("some"));
        assert!(!map.contains_key("other"));
    }
}
