//! Card element types and builders for interactive rich messages.
//!
//! Cards are structured, interactive messages containing buttons, sections,
//! images, and fields. Platform adapters render them to their native format
//! (e.g. Slack blocks, Discord embeds). The [`render_card_as_text`] helper
//! produces a plain-text fallback for adapters that lack rich formatting.

use std::fmt::Write;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// A card is a structured rich message composed of child elements.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardElement {
    /// Optional title displayed at the top of the card.
    pub title: Option<String>,
    /// Ordered list of child elements that make up the card body.
    pub children: Vec<CardChild>,
    /// When set, [`render_card_as_text`] returns this verbatim instead of
    /// building text from the element tree.
    pub fallback_text: Option<String>,
}

/// A child element inside a [`CardElement`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CardChild {
    /// A text section, optionally with an accessory element.
    Section(SectionElement),
    /// A row of interactive action elements (buttons, etc.).
    Actions(ActionsElement),
    /// A horizontal divider line.
    Divider,
    /// An inline image.
    Image(ImageElement),
    /// A set of label/value field pairs.
    Fields(FieldsElement),
    /// A block of text with optional styling.
    Text(TextElement),
}

/// A section element containing text and an optional accessory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SectionElement {
    /// Primary text content of the section.
    pub text: Option<String>,
    /// Optional accessory element displayed alongside the text.
    pub accessory: Option<Box<CardChild>>,
}

/// A container for interactive action elements.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionsElement {
    /// The action elements (buttons, etc.) in this row.
    pub elements: Vec<ActionElement>,
}

/// An individual interactive action element.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActionElement {
    /// A clickable button.
    Button(ButtonElement),
}

/// A clickable button element.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ButtonElement {
    /// Unique identifier for the button (used in callbacks).
    pub id: String,
    /// Display text on the button.
    pub text: String,
    /// Optional payload value attached to the button.
    pub value: Option<String>,
    /// Visual style of the button.
    pub style: ButtonStyle,
    /// Optional URL the button navigates to.
    pub url: Option<String>,
}

/// Visual style for a [`ButtonElement`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ButtonStyle {
    /// Standard / neutral style.
    #[default]
    Default,
    /// Emphasised / call-to-action style.
    Primary,
    /// Destructive / warning style.
    Danger,
}

/// An image element displayed inline in a card.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageElement {
    /// URL of the image.
    pub url: String,
    /// Alt text describing the image.
    pub alt_text: String,
}

/// A set of label/value field pairs displayed in a grid.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldsElement {
    /// The individual fields.
    pub fields: Vec<FieldElement>,
}

/// A single label/value pair inside a [`FieldsElement`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldElement {
    /// The field label.
    pub label: String,
    /// The field value.
    pub value: String,
}

/// A styled text block inside a card.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextElement {
    /// The text content.
    pub text: String,
    /// Rendering style for the text.
    pub style: TextStyle,
}

/// Rendering style for a [`TextElement`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TextStyle {
    /// Render as plain text.
    #[default]
    Plain,
    /// Render as Markdown.
    Markdown,
}

// ---------------------------------------------------------------------------
// Builders – CardElement
// ---------------------------------------------------------------------------

impl CardElement {
    /// Create a new empty card.
    #[must_use]
    pub fn new() -> Self {
        Self { title: None, children: Vec::new(), fallback_text: None }
    }

    /// Set the card title.
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Append a section child.
    #[must_use]
    pub fn section(mut self, section: SectionElement) -> Self {
        self.children.push(CardChild::Section(section));
        self
    }

    /// Append an actions child.
    #[must_use]
    pub fn actions(mut self, actions: ActionsElement) -> Self {
        self.children.push(CardChild::Actions(actions));
        self
    }

    /// Append a divider.
    #[must_use]
    pub fn divider(mut self) -> Self {
        self.children.push(CardChild::Divider);
        self
    }

    /// Append an image child.
    #[must_use]
    pub fn image(mut self, image: ImageElement) -> Self {
        self.children.push(CardChild::Image(image));
        self
    }

    /// Append a fields child.
    #[must_use]
    pub fn fields(mut self, fields: FieldsElement) -> Self {
        self.children.push(CardChild::Fields(fields));
        self
    }

    /// Append a text child.
    #[must_use]
    pub fn text(mut self, text: TextElement) -> Self {
        self.children.push(CardChild::Text(text));
        self
    }

    /// Set explicit fallback text (used by [`render_card_as_text`]).
    #[must_use]
    pub fn fallback_text(mut self, text: impl Into<String>) -> Self {
        self.fallback_text = Some(text.into());
        self
    }
}

impl Default for CardElement {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Builders – ButtonElement
// ---------------------------------------------------------------------------

impl ButtonElement {
    /// Create a new button with the given identifier and display text.
    #[must_use]
    pub fn new(id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            text: text.into(),
            value: None,
            style: ButtonStyle::Default,
            url: None,
        }
    }

    /// Set the callback payload value.
    #[must_use]
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(value.into());
        self
    }

    /// Set the visual style.
    #[must_use]
    pub fn style(mut self, style: ButtonStyle) -> Self {
        self.style = style;
        self
    }

    /// Set the navigation URL.
    #[must_use]
    pub fn url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }
}

// ---------------------------------------------------------------------------
// Plain-text fallback renderer
// ---------------------------------------------------------------------------

/// Render a [`CardElement`] as plain text.
///
/// If the card has [`CardElement::fallback_text`] set, it is returned
/// verbatim.  Otherwise a textual representation is built from the element
/// tree.
#[must_use]
pub fn render_card_as_text(card: &CardElement) -> String {
    if let Some(ref fallback) = card.fallback_text {
        return fallback.clone();
    }

    let mut buf = String::new();

    if let Some(ref title) = card.title {
        let _ = writeln!(buf, "**{title}**");
    }

    for child in &card.children {
        render_child(&mut buf, child);
    }

    // Remove trailing newline for a tidy output.
    while buf.ends_with('\n') {
        buf.pop();
    }

    buf
}

fn render_child(buf: &mut String, child: &CardChild) {
    match child {
        CardChild::Section(section) => {
            if let Some(ref text) = section.text {
                let _ = writeln!(buf, "{text}");
            }
            if let Some(ref accessory) = section.accessory {
                render_child(buf, accessory);
            }
        }
        CardChild::Actions(actions) => {
            for action in &actions.elements {
                match action {
                    ActionElement::Button(button) => {
                        let _ = writeln!(buf, "[Button: {}]", button.text);
                    }
                }
            }
        }
        CardChild::Divider => {
            let _ = writeln!(buf, "---");
        }
        CardChild::Image(image) => {
            let _ = writeln!(buf, "[{}]", image.alt_text);
        }
        CardChild::Fields(fields) => {
            for field in &fields.fields {
                let _ = writeln!(buf, "{}: {}", field.label, field.value);
            }
        }
        CardChild::Text(text) => {
            let _ = writeln!(buf, "{}", text.text);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_constructs_correct_tree() {
        let card = CardElement::new()
            .title("Deploy Report")
            .section(SectionElement { text: Some("All services healthy.".into()), accessory: None })
            .divider()
            .fields(FieldsElement {
                fields: vec![
                    FieldElement { label: "Region".into(), value: "us-east-1".into() },
                    FieldElement { label: "Status".into(), value: "OK".into() },
                ],
            })
            .actions(ActionsElement {
                elements: vec![ActionElement::Button(
                    ButtonElement::new("approve", "Approve")
                        .style(ButtonStyle::Primary)
                        .value("yes"),
                )],
            })
            .image(ImageElement {
                url: "https://example.com/img.png".into(),
                alt_text: "dashboard screenshot".into(),
            })
            .text(TextElement { text: "Footer note".into(), style: TextStyle::Plain });

        assert_eq!(card.title.as_deref(), Some("Deploy Report"));
        assert_eq!(card.children.len(), 6);

        // Verify child order.
        assert!(matches!(card.children[0], CardChild::Section(_)));
        assert!(matches!(card.children[1], CardChild::Divider));
        assert!(matches!(card.children[2], CardChild::Fields(_)));
        assert!(matches!(card.children[3], CardChild::Actions(_)));
        assert!(matches!(card.children[4], CardChild::Image(_)));
        assert!(matches!(card.children[5], CardChild::Text(_)));
    }

    #[test]
    fn render_card_as_text_full() {
        let card = CardElement::new()
            .title("Status")
            .section(SectionElement { text: Some("Everything is fine.".into()), accessory: None })
            .divider()
            .fields(FieldsElement {
                fields: vec![FieldElement { label: "Uptime".into(), value: "99.9%".into() }],
            })
            .actions(ActionsElement {
                elements: vec![ActionElement::Button(ButtonElement::new("ack", "Acknowledge"))],
            });

        let text = render_card_as_text(&card);

        assert!(text.contains("**Status**"));
        assert!(text.contains("Everything is fine."));
        assert!(text.contains("---"));
        assert!(text.contains("Uptime: 99.9%"));
        assert!(text.contains("[Button: Acknowledge]"));
    }

    #[test]
    fn render_card_as_text_returns_fallback() {
        let card = CardElement::new().title("Ignored").fallback_text("custom fallback");

        assert_eq!(render_card_as_text(&card), "custom fallback");
    }

    #[test]
    fn serde_roundtrip() {
        let card = CardElement::new()
            .title("RT")
            .section(SectionElement { text: Some("sec".into()), accessory: None })
            .divider()
            .actions(ActionsElement {
                elements: vec![ActionElement::Button(
                    ButtonElement::new("b1", "Click")
                        .value("v")
                        .style(ButtonStyle::Danger)
                        .url("https://example.com"),
                )],
            })
            .image(ImageElement { url: "https://img.test/a.png".into(), alt_text: "alt".into() })
            .fields(FieldsElement {
                fields: vec![FieldElement { label: "k".into(), value: "v".into() }],
            })
            .text(TextElement { text: "md".into(), style: TextStyle::Markdown });

        let json = serde_json::to_string(&card).expect("serialize");
        let back: CardElement = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(back.title, card.title);
        assert_eq!(back.children.len(), card.children.len());
    }

    #[test]
    fn button_builder_defaults() {
        let btn = ButtonElement::new("id", "text");
        assert_eq!(btn.style, ButtonStyle::Default);
        assert!(btn.value.is_none());
        assert!(btn.url.is_none());
    }

    #[test]
    fn enum_defaults() {
        assert_eq!(ButtonStyle::default(), ButtonStyle::Default);
        assert_eq!(TextStyle::default(), TextStyle::Plain);
    }
}
