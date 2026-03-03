//! Modal element types and builders for interactive form dialogs.
//!
//! Modals are form dialogs containing text inputs, selects, and radio selects.
//! They are opened by `ChatAdapter::open_modal` and processed by modal event
//! handlers. Platform adapters render them to their native format (e.g. Slack
//! views, Discord modals).

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// A modal dialog composed of form input elements.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModalElement {
    /// Unique identifier used to route modal submission callbacks.
    pub callback_id: String,
    /// Title displayed at the top of the modal.
    pub title: String,
    /// Optional label for the submit button (defaults to platform default).
    pub submit_label: Option<String>,
    /// Ordered list of child form elements.
    pub children: Vec<ModalChild>,
    /// Optional opaque metadata passed through submission callbacks.
    pub private_metadata: Option<String>,
    /// When `true`, the adapter should fire a close/cancel event.
    pub notify_on_close: bool,
}

/// A child element inside a [`ModalElement`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ModalChild {
    /// A text input field.
    TextInput(TextInputElement),
    /// A dropdown select menu.
    Select(SelectElement),
    /// A radio-button select group.
    RadioSelect(RadioSelectElement),
}

/// A single-line or multi-line text input element.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextInputElement {
    /// Unique identifier for the input (used in submission payloads).
    pub id: String,
    /// Label displayed above the input.
    pub label: String,
    /// Placeholder text shown when the input is empty.
    pub placeholder: Option<String>,
    /// Pre-filled value.
    pub initial_value: Option<String>,
    /// When `true`, renders as a multi-line text area.
    pub multiline: bool,
    /// When `true`, the field may be left blank on submission.
    pub optional: bool,
}

/// A dropdown select element.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectElement {
    /// Unique identifier for the select (used in submission payloads).
    pub id: String,
    /// Label displayed above the select.
    pub label: String,
    /// Placeholder text shown when no option is selected.
    pub placeholder: Option<String>,
    /// Available options.
    pub options: Vec<SelectOption>,
    /// Value of the initially selected option.
    pub initial_option: Option<String>,
}

/// A single option inside a [`SelectElement`] or [`RadioSelectElement`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectOption {
    /// Display label for the option.
    pub label: String,
    /// Machine-readable value for the option.
    pub value: String,
}

/// A radio-button select group.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RadioSelectElement {
    /// Unique identifier for the radio group (used in submission payloads).
    pub id: String,
    /// Label displayed above the radio group.
    pub label: String,
    /// Available options.
    pub options: Vec<SelectOption>,
    /// Value of the initially selected option.
    pub initial_option: Option<String>,
}

// ---------------------------------------------------------------------------
// Builders – ModalElement
// ---------------------------------------------------------------------------

impl ModalElement {
    /// Create a new modal with the given callback identifier and title.
    #[must_use]
    pub fn new(callback_id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            callback_id: callback_id.into(),
            title: title.into(),
            submit_label: None,
            children: Vec::new(),
            private_metadata: None,
            notify_on_close: false,
        }
    }

    /// Set the submit button label.
    #[must_use]
    pub fn submit_label(mut self, label: impl Into<String>) -> Self {
        self.submit_label = Some(label.into());
        self
    }

    /// Append a text input child element.
    #[must_use]
    pub fn text_input(mut self, input: TextInputElement) -> Self {
        self.children.push(ModalChild::TextInput(input));
        self
    }

    /// Append a select child element.
    #[must_use]
    pub fn select(mut self, select: SelectElement) -> Self {
        self.children.push(ModalChild::Select(select));
        self
    }

    /// Append a radio select child element.
    #[must_use]
    pub fn radio_select(mut self, radio: RadioSelectElement) -> Self {
        self.children.push(ModalChild::RadioSelect(radio));
        self
    }

    /// Set opaque metadata passed through submission callbacks.
    #[must_use]
    pub fn private_metadata(mut self, metadata: impl Into<String>) -> Self {
        self.private_metadata = Some(metadata.into());
        self
    }

    /// Set whether a close/cancel event should be fired.
    #[must_use]
    pub fn notify_on_close(mut self, notify: bool) -> Self {
        self.notify_on_close = notify;
        self
    }
}

// ---------------------------------------------------------------------------
// Builders – TextInputElement
// ---------------------------------------------------------------------------

impl TextInputElement {
    /// Create a new text input with the given identifier and label.
    #[must_use]
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            placeholder: None,
            initial_value: None,
            multiline: false,
            optional: false,
        }
    }

    /// Set placeholder text.
    #[must_use]
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    /// Set the initial (pre-filled) value.
    #[must_use]
    pub fn initial_value(mut self, value: impl Into<String>) -> Self {
        self.initial_value = Some(value.into());
        self
    }

    /// Set whether the input renders as a multi-line text area.
    #[must_use]
    pub fn multiline(mut self, multiline: bool) -> Self {
        self.multiline = multiline;
        self
    }

    /// Set whether the field is optional.
    #[must_use]
    pub fn optional(mut self, optional: bool) -> Self {
        self.optional = optional;
        self
    }
}

// ---------------------------------------------------------------------------
// Builders – SelectElement
// ---------------------------------------------------------------------------

impl SelectElement {
    /// Create a new select with the given identifier and label.
    #[must_use]
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            placeholder: None,
            options: Vec::new(),
            initial_option: None,
        }
    }

    /// Set placeholder text.
    #[must_use]
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    /// Append an option.
    #[must_use]
    pub fn option(mut self, option: SelectOption) -> Self {
        self.options.push(option);
        self
    }

    /// Set the initially selected option by value.
    #[must_use]
    pub fn initial_option(mut self, value: impl Into<String>) -> Self {
        self.initial_option = Some(value.into());
        self
    }
}

// ---------------------------------------------------------------------------
// Builders – SelectOption
// ---------------------------------------------------------------------------

impl SelectOption {
    /// Create a new option with the given label and value.
    #[must_use]
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self { label: label.into(), value: value.into() }
    }
}

// ---------------------------------------------------------------------------
// Builders – RadioSelectElement
// ---------------------------------------------------------------------------

impl RadioSelectElement {
    /// Create a new radio select group with the given identifier and label.
    #[must_use]
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self { id: id.into(), label: label.into(), options: Vec::new(), initial_option: None }
    }

    /// Append an option.
    #[must_use]
    pub fn option(mut self, option: SelectOption) -> Self {
        self.options.push(option);
        self
    }

    /// Set the initially selected option by value.
    #[must_use]
    pub fn initial_option(mut self, value: impl Into<String>) -> Self {
        self.initial_option = Some(value.into());
        self
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_modal_construction() {
        let modal = ModalElement::new("cb_1", "My Modal");
        assert_eq!(modal.callback_id, "cb_1");
        assert_eq!(modal.title, "My Modal");
        assert!(modal.submit_label.is_none());
        assert!(modal.children.is_empty());
        assert!(modal.private_metadata.is_none());
        assert!(!modal.notify_on_close);
    }

    #[test]
    fn builder_chaining() {
        let modal = ModalElement::new("cb_settings", "Settings")
            .submit_label("Save")
            .private_metadata("{\"v\":1}")
            .notify_on_close(true)
            .text_input(
                TextInputElement::new("name", "Your Name")
                    .placeholder("Enter name")
                    .initial_value("Alice")
                    .multiline(false)
                    .optional(false),
            )
            .select(
                SelectElement::new("color", "Favourite Colour")
                    .placeholder("Pick one")
                    .option(SelectOption::new("Red", "red"))
                    .option(SelectOption::new("Blue", "blue"))
                    .initial_option("blue"),
            )
            .radio_select(
                RadioSelectElement::new("size", "T-Shirt Size")
                    .option(SelectOption::new("Small", "s"))
                    .option(SelectOption::new("Medium", "m"))
                    .option(SelectOption::new("Large", "l"))
                    .initial_option("m"),
            );

        assert_eq!(modal.submit_label.as_deref(), Some("Save"));
        assert_eq!(modal.private_metadata.as_deref(), Some("{\"v\":1}"));
        assert!(modal.notify_on_close);
        assert_eq!(modal.children.len(), 3);

        // Verify child types
        assert!(matches!(modal.children[0], ModalChild::TextInput(_)));
        assert!(matches!(modal.children[1], ModalChild::Select(_)));
        assert!(matches!(modal.children[2], ModalChild::RadioSelect(_)));

        // Verify text input details
        if let ModalChild::TextInput(ref input) = modal.children[0] {
            assert_eq!(input.id, "name");
            assert_eq!(input.label, "Your Name");
            assert_eq!(input.placeholder.as_deref(), Some("Enter name"));
            assert_eq!(input.initial_value.as_deref(), Some("Alice"));
            assert!(!input.multiline);
            assert!(!input.optional);
        }

        // Verify select details
        if let ModalChild::Select(ref select) = modal.children[1] {
            assert_eq!(select.id, "color");
            assert_eq!(select.options.len(), 2);
            assert_eq!(select.options[0].label, "Red");
            assert_eq!(select.options[0].value, "red");
            assert_eq!(select.initial_option.as_deref(), Some("blue"));
        }

        // Verify radio select details
        if let ModalChild::RadioSelect(ref radio) = modal.children[2] {
            assert_eq!(radio.id, "size");
            assert_eq!(radio.options.len(), 3);
            assert_eq!(radio.initial_option.as_deref(), Some("m"));
        }
    }

    #[test]
    fn serde_roundtrip() {
        let original = ModalElement::new("cb_rt", "Roundtrip")
            .submit_label("Go")
            .notify_on_close(true)
            .text_input(
                TextInputElement::new("field1", "Field 1")
                    .placeholder("type here")
                    .multiline(true)
                    .optional(true),
            )
            .select(
                SelectElement::new("sel1", "Select 1")
                    .option(SelectOption::new("A", "a"))
                    .initial_option("a"),
            )
            .radio_select(
                RadioSelectElement::new("rad1", "Radio 1")
                    .option(SelectOption::new("X", "x"))
                    .option(SelectOption::new("Y", "y")),
            );

        let json = serde_json::to_string(&original).expect("serialize");
        let restored: ModalElement = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(restored.callback_id, original.callback_id);
        assert_eq!(restored.title, original.title);
        assert_eq!(restored.submit_label, original.submit_label);
        assert_eq!(restored.notify_on_close, original.notify_on_close);
        assert_eq!(restored.children.len(), original.children.len());

        // Deep check on text input child
        if let (ModalChild::TextInput(orig), ModalChild::TextInput(rest)) =
            (&original.children[0], &restored.children[0])
        {
            assert_eq!(orig.id, rest.id);
            assert_eq!(orig.label, rest.label);
            assert_eq!(orig.placeholder, rest.placeholder);
            assert_eq!(orig.initial_value, rest.initial_value);
            assert_eq!(orig.multiline, rest.multiline);
            assert_eq!(orig.optional, rest.optional);
        } else {
            panic!("expected TextInput children");
        }
    }

    #[test]
    fn serde_roundtrip_empty_modal() {
        let original = ModalElement::new("cb_empty", "Empty");
        let json = serde_json::to_string(&original).expect("serialize");
        let restored: ModalElement = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(restored.callback_id, "cb_empty");
        assert_eq!(restored.title, "Empty");
        assert!(restored.children.is_empty());
        assert!(!restored.notify_on_close);
    }
}
