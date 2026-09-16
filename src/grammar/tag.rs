//! The productions above `Value`: `Top`, `Annotation`, `TagOpen`, `TagClose`
//! and the attribute list.
//!
//! Read this beside `reference/src/grammar/tag.pegjs`. Each function is one
//! rule, in the order the file declares them, and the alternation order inside
//! a function is the alternation order in the grammar. That order is not
//! stylistic: `Annotation` is tried before `TagOpen`, so `{% test=1 %}` is an
//! annotation and never a tag named `test`.

use super::cursor::Cursor;
use super::{Attribute, AttributeSpan, TagItem};
use crate::ast::Value;

impl Cursor<'_> {
    /// `Top = TopLevelValue / Annotation / TagOpen / TagClose`
    ///
    /// PEG ordered choice, and nothing more: the first alternative that
    /// matches wins, even if a later one would have consumed more of the
    /// input. Whether what it matched is the *whole* body is checked by the
    /// caller, which is how upstream works and why `{% foo=1 bar %}` is an
    /// error rather than a tag.
    /// The spans travel beside the item rather than inside it, one per
    /// attribute in authored order. An alternative that carries no attributes
    /// reports none.
    pub(crate) fn top(&mut self) -> Option<(TagItem, Vec<AttributeSpan>)> {
        if let Some(item) = self.top_level_value() {
            return Some((item, Vec::new()));
        }
        if let Some(item) = self.annotation() {
            return Some(item);
        }
        if let Some(item) = self.tag_open() {
            return Some(item);
        }
        self.tag_close().map(|item| (item, Vec::new()))
    }

    /// `TopLevelValue = (Variable / Function)`
    ///
    /// A bare value in tag position: `{% $foo %}` or `{% equals(1, 1) %}`.
    /// Upstream labels the result `variable` for both, and a host renders it by
    /// resolving the value rather than by looking up a tag.
    fn top_level_value(&mut self) -> Option<TagItem> {
        if let Some(variable) = self.variable() {
            return Some(TagItem::Variable(variable));
        }
        let function = self.function()?;
        Some(TagItem::Variable(function))
    }

    /// `Annotation = TagAttributes _*`
    ///
    /// Attributes with no tag name, which annotate the node they follow.
    fn annotation(&mut self) -> Option<(TagItem, Vec<AttributeSpan>)> {
        let start = self.pos();
        let Some(items) = self.tag_attributes() else {
            self.reset(start);
            return None;
        };
        self.whitespace_star();
        let (attributes, spans): (Vec<_>, Vec<_>) = items.into_iter().unzip();
        Some((TagItem::Annotation { attributes }, spans))
    }

    /// `TagOpen = TagName _* primary? TagAttributes? _* '/'?`
    ///
    /// Everything after the name is optional, so this rule cannot fail once
    /// the name matches -- it can only match less than the caller needs, and
    /// the end-of-input check turns that into the error.
    fn tag_open(&mut self) -> Option<(TagItem, Vec<AttributeSpan>)> {
        let start = self.pos();
        let Some(name) = self.tag_name() else {
            self.reset(start);
            return None;
        };
        self.whitespace_star();

        // `primary:( value:Value _? )?` -- one optional whitespace, not `_*`.
        // Two spaces between the primary value and the first attribute is a
        // syntax error upstream, and is one here.
        //
        // The span closes before the optional whitespace, so it covers the
        // value and not the gap after it.
        let primary = {
            let mark = self.pos();
            if let Some(value) = self.value() {
                let span = mark..self.pos();
                self.whitespace();
                Some((value, span))
            } else {
                self.reset(mark);
                None
            }
        };

        let mut items = self.tag_attributes().unwrap_or_default();
        self.whitespace_star();
        let self_closing = self.literal("/");

        // Upstream unshifts the primary value as an attribute named `primary`
        // under a bare `if (primary)`, so a falsy primary is parsed and then
        // dropped: `{% foo 0 %}` and `{% foo null %}` carry no attributes at
        // all. That is a quirk, and porting it is the point.
        //
        // Its name is synthetic and its value is written, so `all` and `value`
        // are the same range: there is no `primary=` in the source to cover.
        if let Some((primary, span)) = primary
            && primary.is_truthy()
        {
            items.insert(
                0,
                (
                    Attribute::Attribute {
                        name: "primary".to_string(),
                        value: primary,
                    },
                    AttributeSpan {
                        all: span.clone(),
                        value: Some(span),
                    },
                ),
            );
        }

        let (attributes, spans): (Vec<_>, Vec<_>) = items.into_iter().unzip();
        Some((
            TagItem::TagOpen {
                name,
                attributes,
                self_closing,
            },
            spans,
        ))
    }

    /// `TagClose = '/' TagName`
    ///
    /// Nothing may follow the name, which is why `{% /foo test=1 %}` is an
    /// error rather than a close tag with an ignored attribute.
    fn tag_close(&mut self) -> Option<TagItem> {
        let start = self.pos();
        if !self.literal("/") {
            self.reset(start);
            return None;
        }
        let Some(name) = self.tag_name() else {
            self.reset(start);
            return None;
        };
        Some(TagItem::TagClose { name })
    }

    /// `TagName 'tag name' = Identifier`
    fn tag_name(&mut self) -> Option<String> {
        self.enter_named("tag name");
        let name = self.identifier().map(ToString::to_string);
        self.leave_named();
        name
    }

    /// `TagAttributes = TagAttributesItem (_+ TagAttributesItem)*`
    ///
    /// At least one item, separated by whitespace. Returning `None` for "no
    /// attributes at all" is what lets `TagOpen` tell `{% foo %}` from
    /// `{% foo x=1 %}`; upstream carries the same distinction as `null` versus
    /// an array, and both of its consumers treat the two alike.
    fn tag_attributes(&mut self) -> Option<Vec<(Attribute, AttributeSpan)>> {
        let head = self.tag_attributes_item()?;
        let mut items = vec![head];
        loop {
            let mark = self.pos();
            if !self.whitespace_plus() {
                self.reset(mark);
                break;
            }
            let Some(item) = self.tag_attributes_item() else {
                self.reset(mark);
                break;
            };
            items.push(item);
        }
        Some(items)
    }

    /// `TagAttributesItem = TagShortcutId / TagShortcutClass / TagAttribute`
    fn tag_attributes_item(&mut self) -> Option<(Attribute, AttributeSpan)> {
        if let Some(item) = self.tag_shortcut_id() {
            return Some(item);
        }
        if let Some(item) = self.tag_shortcut_class() {
            return Some(item);
        }
        self.tag_attribute()
    }

    /// `TagShortcutId 'id' = '#' Identifier`
    ///
    /// The shortcut is an ordinary attribute named `id` whose value is a
    /// string, not a distinct kind of item. Only the class shortcut gets its
    /// own shape, because a node collects classes into a set.
    fn tag_shortcut_id(&mut self) -> Option<(Attribute, AttributeSpan)> {
        let start = self.pos();
        self.enter_named("id");
        let name = if self.literal("#") {
            self.identifier()
        } else {
            None
        };
        self.leave_named();

        if let Some(name) = name {
            return Some((
                Attribute::Attribute {
                    name: "id".to_string(),
                    value: Value::String(name.to_string()),
                },
                AttributeSpan {
                    all: start..self.pos(),
                    value: None,
                },
            ));
        }
        self.reset(start);
        None
    }

    /// `TagShortcutClass 'class' = '.' Identifier`
    fn tag_shortcut_class(&mut self) -> Option<(Attribute, AttributeSpan)> {
        let start = self.pos();
        self.enter_named("class");
        let name = if self.literal(".") {
            self.identifier()
        } else {
            None
        };
        self.leave_named();

        if let Some(name) = name {
            return Some((
                Attribute::Class {
                    name: name.to_string(),
                },
                AttributeSpan {
                    all: start..self.pos(),
                    value: None,
                },
            ));
        }
        self.reset(start);
        None
    }

    /// `TagAttribute = Identifier '=' Value`
    ///
    /// No whitespace is permitted around the `=`, in either direction.
    fn tag_attribute(&mut self) -> Option<(Attribute, AttributeSpan)> {
        let start = self.pos();
        let Some(name) = self.identifier() else {
            self.reset(start);
            return None;
        };
        if !self.literal("=") {
            self.reset(start);
            return None;
        }
        let value_start = self.pos();
        let Some(value) = self.value() else {
            self.reset(start);
            return None;
        };
        let end = self.pos();
        Some((
            Attribute::Attribute {
                name: name.to_string(),
                value,
            },
            AttributeSpan {
                all: start..end,
                value: Some(value_start..end),
            },
        ))
    }
}
