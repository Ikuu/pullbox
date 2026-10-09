//! Reading and writing of `ComicInfo.xml`, the metadata format used by ComicRack.
//!
//! Elements are kept in document order. Simple text elements are exposed by
//! name; complex elements (e.g. `<Pages>`) and elements this app doesn't know
//! about are kept verbatim so that saving never loses data.

pub mod fields;

use std::fmt::Write;

use anyhow::{Context, Result};

/// The file name ComicRack stores metadata under inside an archive.
pub const FILE_NAME: &str = "ComicInfo.xml";

#[derive(Clone, Debug, PartialEq)]
enum Value {
    Text(String),
    /// Original XML of an element that has child elements.
    Raw(String),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ComicInfo {
    elements: Vec<(String, Value)>,
}

impl ComicInfo {
    pub fn parse(xml: &str) -> Result<Self> {
        // Some writers emit a BOM, which roxmltree rejects.
        let xml = xml.trim_start_matches('\u{feff}');
        let doc = roxmltree::Document::parse(xml).context("invalid ComicInfo.xml")?;

        let elements = doc
            .root_element()
            .children()
            .filter(roxmltree::Node::is_element)
            .map(|node| {
                let name = node.tag_name().name().to_owned();
                let value = if node.children().any(|c| c.is_element()) {
                    Value::Raw(xml[node.range()].to_owned())
                } else {
                    Value::Text(node.text().unwrap_or_default().trim().to_owned())
                };
                (name, value)
            })
            .collect();

        Ok(Self { elements })
    }

    /// Text of a simple element, if present and non-empty.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.elements.iter().find_map(|(n, v)| match v {
            Value::Text(text) if n == name && !text.is_empty() => Some(text.as_str()),
            _ => None,
        })
    }

    pub fn get_i32(&self, name: &str) -> Option<i32> {
        self.get(name)?.parse().ok()
    }

    /// Sets a simple element. An empty (or whitespace) value removes it.
    pub fn set(&mut self, name: &str, value: &str) {
        let value = value.trim();
        let existing = self.elements.iter().position(|(n, _)| n == name);
        match (existing, value.is_empty()) {
            (Some(i), true) => {
                self.elements.remove(i);
            }
            (Some(i), false) => self.elements[i].1 = Value::Text(value.to_owned()),
            (None, true) => {}
            (None, false) => self.elements.push((name.to_owned(), Value::Text(value.to_owned()))),
        }
    }

    /// Serialises to XML: known fields in ComicRack's order, then unknown
    /// simple elements, then complex elements such as `<Pages>`.
    pub fn to_xml(&self) -> String {
        let mut known: Vec<_> = self
            .elements
            .iter()
            .filter_map(|(name, value)| match value {
                Value::Text(text) => fields::order_of(name).map(|order| (order, name, text)),
                Value::Raw(_) => None,
            })
            .collect();
        known.sort_by_key(|(order, ..)| *order);

        let mut xml = String::from(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
             <ComicInfo xmlns:xsd=\"http://www.w3.org/2001/XMLSchema\" \
             xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">\n",
        );
        for (_, name, text) in known {
            write_text_element(&mut xml, name, text);
        }
        for (name, value) in &self.elements {
            if let Value::Text(text) = value
                && fields::order_of(name).is_none()
            {
                write_text_element(&mut xml, name, text);
            }
        }
        for (_, value) in &self.elements {
            if let Value::Raw(raw) = value {
                let _ = writeln!(xml, "  {raw}");
            }
        }
        xml.push_str("</ComicInfo>\n");
        xml
    }
}

fn write_text_element(xml: &mut String, name: &str, text: &str) {
    if !text.is_empty() {
        let _ = writeln!(xml, "  <{name}>{}</{name}>", escape(text));
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0"?>
<ComicInfo xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <Series>Saga</Series>
  <Number>12</Number>
  <Year>2013</Year>
  <Summary></Summary>
  <GTIN>123</GTIN>
  <Pages><Page Image="0" Type="FrontCover"/></Pages>
</ComicInfo>"#;

    #[test]
    fn parses_simple_and_complex_elements() {
        let info = ComicInfo::parse(SAMPLE).unwrap();
        assert_eq!(info.get("Series"), Some("Saga"));
        assert_eq!(info.get_i32("Year"), Some(2013));
        assert_eq!(info.get("Summary"), None);
        assert_eq!(info.get("Pages"), None);
    }

    #[test]
    fn round_trip_keeps_unknown_and_complex_elements() {
        let mut info = ComicInfo::parse(SAMPLE).unwrap();
        info.set("Title", "Chapter <Twelve> & more");
        info.set("Year", "");

        let reparsed = ComicInfo::parse(&info.to_xml()).unwrap();
        assert_eq!(reparsed.get("Title"), Some("Chapter <Twelve> & more"));
        assert_eq!(reparsed.get("Year"), None);
        assert_eq!(reparsed.get("GTIN"), Some("123"));
        assert!(info.to_xml().contains(r#"<Page Image="0" Type="FrontCover"/>"#));

        // Title comes before Series in ComicRack's order.
        let xml = info.to_xml();
        assert!(xml.find("<Title>").unwrap() < xml.find("<Series>").unwrap());
    }
}
