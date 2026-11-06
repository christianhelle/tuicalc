//! Small helpers over quick-xml.

use std::borrow::Cow;

use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};

use super::WorkbookError;

pub fn xml_error(error: impl std::fmt::Display) -> WorkbookError {
    WorkbookError::Xml(error.to_string())
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

/// As `attr`, for values with no entities in them such as references and style numbers, which it
/// doesn't decode or copy. Reading these is the hottest path there is when opening a large sheet.
pub fn plain_attr<'a>(element: &'a BytesStart<'_>, local: &str) -> Option<Cow<'a, str>> {
    element
        .attributes()
        .with_checks(false)
        .flatten()
        .find_map(|attribute| {
            let key = attribute.key.local_name();
            (AsRef::<str>::as_ref(&key) == local).then_some(attribute.value)
        })
}

pub fn local_name(element: &BytesStart<'_>) -> String {
    AsRef::<str>::as_ref(&element.local_name()).to_string()
}

/// Appends the text carried by a text-like event; other events are ignored.
pub fn push_text(event: &Event<'_>, out: &mut String) -> Result<(), WorkbookError> {
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

/// How far a reader has got through its source, as a byte offset.
pub fn position(reader: &Reader<&[u8]>) -> usize {
    reader.buffer_position() as usize
}

/// Copies source XML through to `out`, leaving out or replacing the spans that change, so that
/// every other byte stays as it was.
pub struct Splice<'a> {
    source: &'a str,
    pub out: String,
    copied_to: usize,
}

impl<'a> Splice<'a> {
    pub fn new(source: &'a str, extra: usize) -> Self {
        Self {
            source,
            out: String::with_capacity(source.len() + extra),
            copied_to: 0,
        }
    }

    /// Copies source bytes up to `end`, then skips ahead to `resume`.
    pub fn copy_until(&mut self, end: usize, resume: usize) {
        self.out.push_str(&self.source[self.copied_to..end]);
        self.copied_to = resume;
    }

    /// The output, with the rest of the source copied through.
    pub fn finish(mut self) -> String {
        let end = self.source.len();
        self.copy_until(end, end);
        self.out
    }
}

#[cfg(test)]
mod tests {
    use super::Splice;

    #[test]
    fn a_splice_keeps_what_it_is_not_told_to_skip() {
        let mut splice = Splice::new("<a><b/><c/></a>", 0);
        splice.copy_until(3, 7);
        splice.out.push_str("<x/>");
        assert_eq!(splice.finish(), "<a><x/><c/></a>");
    }

    #[test]
    fn an_untouched_splice_copies_everything() {
        assert_eq!(Splice::new("<a/>", 0).finish(), "<a/>");
    }
}
