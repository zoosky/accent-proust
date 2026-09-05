//! The renderable tree, as the JavaScript objects a Markdoc renderer expects.
//!
//! Upstream's `Tag` is a class carrying `$$mdtype: 'Tag'`, and every renderer it
//! ships branches on that field (`reference/src/renderers/react/react.ts`). A
//! derived serialisation cannot produce it, which is why this mapping is written
//! out rather than delegated to serde -- and why writing it here rather than in
//! the library keeps the library at three dependencies and adds no public type.
//!
//! # Why the walk is iterative
//!
//! A renderable tree is built from an attacker-controlled document, and the
//! library promises panic-freedom for every traversal of one. A stack overflow
//! aborts rather than panics, and in WebAssembly it traps and poisons the
//! instance, so the promise matters more here than it does natively. The walk
//! below is a worklist for the same reason the library's own `Drop`, `Clone`,
//! `PartialEq` and `Debug` are.
//!
//! Values come back in post-order: a step that builds a container is pushed
//! first and popped last, after the steps for everything it contains have left
//! their results on `values`.

use accent_proust::renderable::{RenderableTreeNode, RenderableTreeNodes, Scalar, Tag};
use indexmap::IndexMap;
use js_sys::{Array, Object, Reflect};
use wasm_bindgen::JsValue;

/// One unit of work: either a source node to convert, or a container to close
/// over results already on the value stack.
enum Step<'a> {
    /// Convert a tree node.
    Node(&'a RenderableTreeNode),
    /// Convert a scalar.
    Value(&'a Scalar),
    /// Close a tag over its attribute values and children.
    Tag(&'a Tag),
    /// Close an array over the given number of elements.
    Array(usize),
    /// Close an object over one value per key, in key order.
    Object(&'a IndexMap<String, Scalar>),
}

/// How many values a `RenderableTreeNodes` leaves on the stack.
///
/// The enum is `#[non_exhaustive]`, so a variant added upstream contributes
/// nothing rather than desynchronising the count from what
/// [`close_tag`] consumes. Both sides call this function for that reason.
fn width(nodes: &RenderableTreeNodes) -> usize {
    match nodes {
        RenderableTreeNodes::One(_) => 1,
        RenderableTreeNodes::Many(list) => list.len(),
        _ => 0,
    }
}

/// Convert a list of tree nodes into a JavaScript array.
pub(crate) fn nodes(list: &[RenderableTreeNode]) -> Array {
    let out = Array::new();
    for item in list {
        out.push(&node(item));
    }
    out
}

/// Convert one tree node.
pub(crate) fn node(root: &RenderableTreeNode) -> JsValue {
    let mut steps = vec![Step::Node(root)];
    let mut values: Vec<JsValue> = Vec::new();

    while let Some(step) = steps.pop() {
        match step {
            Step::Node(RenderableTreeNode::Scalar(value)) => steps.push(Step::Value(value)),
            Step::Node(RenderableTreeNode::Tag(tag)) => open_tag(&mut steps, tag),
            // `RenderableTreeNode` is `#[non_exhaustive]`. A variant this crate
            // has not been taught renders as `null`, which is what upstream's
            // renderers already do with a value they cannot place.
            Step::Node(_) => values.push(JsValue::NULL),
            Step::Value(value) => open_value(&mut steps, &mut values, value),
            Step::Tag(tag) => {
                let object = close_tag(&mut values, tag);
                values.push(object);
            }
            Step::Array(len) => {
                let items = take(&mut values, len);
                let array = Array::new();
                for item in items {
                    array.push(&item);
                }
                values.push(array.into());
            }
            Step::Object(map) => {
                let items = take(&mut values, map.len());
                let object = Object::new();
                for (key, value) in map.keys().zip(items) {
                    set(&object, key, &value);
                }
                values.push(object.into());
            }
        }
    }

    values.pop().unwrap_or(JsValue::NULL)
}

/// Schedule a tag: its own closing step first, then everything it contains, so
/// the contents are popped and converted before the tag closes over them.
///
/// Attributes are scheduled ahead of children because [`close_tag`] reads them
/// in that order. Each group is pushed in reverse so it pops in source order,
/// which is what keeps attribute order the authored order.
fn open_tag<'a>(steps: &mut Vec<Step<'a>>, tag: &'a Tag) {
    steps.push(Step::Tag(tag));
    for child in tag.children.iter().rev() {
        steps.push(Step::Node(child));
    }
    for value in tag.attributes.values().rev() {
        match value {
            RenderableTreeNodes::One(node) => steps.push(Step::Node(node)),
            RenderableTreeNodes::Many(list) => {
                for node in list.iter().rev() {
                    steps.push(Step::Node(node));
                }
            }
            _ => {}
        }
    }
}

/// Convert a scalar in place when it is a leaf, or schedule its contents.
#[allow(
    clippy::match_same_arms,
    reason = "`Null` and the wildcard both produce `null` and say different things: one is the document's null, the other is a variant added to a `#[non_exhaustive]` enum that this crate has not been taught"
)]
fn open_value<'a>(steps: &mut Vec<Step<'a>>, values: &mut Vec<JsValue>, value: &'a Scalar) {
    match value {
        Scalar::Null => values.push(JsValue::NULL),
        Scalar::Boolean(flag) => values.push(JsValue::from_bool(*flag)),
        // The engine formats the number, so this crate does not have to
        // reimplement ECMAScript's `ToString`. A non-finite value becomes `NaN`
        // or an infinity here and `null` under `JSON.stringify`, which is what
        // upstream produces for the same tree.
        Scalar::Number(number) => values.push(JsValue::from_f64(*number)),
        Scalar::String(text) => values.push(JsValue::from_str(text)),
        Scalar::Array(items) => {
            steps.push(Step::Array(items.len()));
            for item in items.iter().rev() {
                steps.push(Step::Value(item));
            }
        }
        Scalar::Object(map) => {
            steps.push(Step::Object(map));
            for item in map.values().rev() {
                steps.push(Step::Value(item));
            }
        }
        _ => values.push(JsValue::NULL),
    }
}

/// Build the tag object from the values its contents left behind.
///
/// Field order is upstream's declaration order in `reference/src/tag.ts`, so
/// `JSON.stringify` over this object and over upstream's produces the same
/// bytes rather than the same data.
fn close_tag(values: &mut Vec<JsValue>, tag: &Tag) -> JsValue {
    let attribute_width: usize = tag.attributes.values().map(width).sum();
    let mut taken = take(values, attribute_width + tag.children.len()).into_iter();

    let attributes = Object::new();
    for (key, value) in &tag.attributes {
        let converted = match value {
            RenderableTreeNodes::One(_) => taken.next().unwrap_or(JsValue::NULL),
            RenderableTreeNodes::Many(list) => {
                let array = Array::new();
                for _ in list {
                    array.push(&taken.next().unwrap_or(JsValue::NULL));
                }
                array.into()
            }
            _ => JsValue::NULL,
        };
        set(&attributes, key, &converted);
    }

    let children = Array::new();
    for child in taken {
        children.push(&child);
    }

    let object = Object::new();
    set(&object, "$$mdtype", &JsValue::from_str("Tag"));
    set(&object, "name", &JsValue::from_str(&tag.name));
    set(&object, "attributes", &attributes);
    set(&object, "children", &children);
    object.into()
}

/// Take the last `count` values, oldest first.
///
/// A short stack is impossible -- every container schedules exactly the steps
/// it later consumes -- but it is handled rather than indexed, because
/// `indexing_slicing` is denied here for the reason it is denied in the
/// library: a proof that holds today is not a promise.
fn take(values: &mut Vec<JsValue>, count: usize) -> Vec<JsValue> {
    match values.len().checked_sub(count) {
        Some(start) => values.split_off(start),
        None => std::mem::take(values),
    }
}

/// Set a property, discarding the result.
///
/// `Reflect::set` fails only on a frozen or proxied target, and both objects
/// here were created a few lines above.
fn set(target: &Object, key: &str, value: &JsValue) {
    let _ = Reflect::set(target, &JsValue::from_str(key), value);
}
