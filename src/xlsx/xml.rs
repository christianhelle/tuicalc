//! Small helpers over quick-xml.

use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event};

use super::XlsxError;

pub fn xml_error(error: impl std::fmt::Display) -> XlsxError {
    XlsxError::Xml(error.to_string())
}

/// The value of the attribute with this local name, ignoring any namespace prefix.
pub fn attr(element: &BytesStart<'_>, local: &str) -> Option<String> {
    element.attributes().flatten().find_map(|attribute| {
        let key = attribute.key.local_name();
        let matches = AsRef::<str>::as_ref(&key) == local;
        matches
            .then(|| attribute.normalized_value(XmlVersion::Implicit1_0).ok())
            .flatten()
            .map(|value| value.into_owned())
    })
}

pub fn local_name(element: &BytesStart<'_>) -> String {
    AsRef::<str>::as_ref(&element.local_name()).to_string()
}

/// Appends the text carried by a text-like event; other events are ignored.
pub fn push_text(event: &Event<'_>, out: &mut String) -> Result<(), XlsxError> {
    match event {
        Event::Text(text) => out.push_str(&text.xml10_content()),
        Event::CData(data) => out.push_str(data),
        Event::GeneralRef(reference) => {
            if let Some(ch) = reference.resolve_char_ref().map_err(xml_error)? {
                out.push(ch);
            } else {
                let entity: &str = reference;
                let resolved = quick_xml::escape::resolve_predefined_entity(entity)
                    .ok_or_else(|| xml_error(format!("unknown entity &{entity};")))?;
                out.push_str(resolved);
            }
        }
        _ => {}
    }
    Ok(())
}
