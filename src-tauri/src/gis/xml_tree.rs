//! Minimal namespace-agnostic XML tree used by the KML/GPX readers.
//!
//! Full XML toolchains are unnecessary for these two formats: we only need
//! element names, attributes and concatenated text, ignoring namespaces.

use std::collections::HashMap;

#[derive(Debug, Default, Clone)]
pub struct XmlElement {
    /// Local element name, lower-cased (namespace prefix stripped).
    pub name: String,
    pub attrs: HashMap<String, String>,
    pub text: String,
    pub children: Vec<XmlElement>,
}

impl XmlElement {
    pub fn attr(&self, key: &str) -> Option<&str> {
        self.attrs.get(key).map(String::as_str)
    }

    pub fn child(&self, name: &str) -> Option<&XmlElement> {
        self.children.iter().find(|c| c.name == name)
    }

    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a XmlElement> {
        self.children.iter().filter(move |c| c.name == name)
    }

    /// Depth-first search for the first descendant element with this name.
    pub fn find_first(&self, name: &str) -> Option<&XmlElement> {
        if self.name == name {
            return Some(self);
        }
        self.children.iter().find_map(|c| c.find_first(name))
    }

    /// Depth-first collection of every descendant element with this name.
    /// Returned references live as long as `self`, independent of `out`.
    pub fn find_all<'a>(&'a self, name: &str, out: &mut Vec<&'a XmlElement>) {
        if self.name == name {
            out.push(self);
        }
        for c in &self.children {
            c.find_all(name, out);
        }
    }
}

/// Parse a document into its top-level elements.
pub fn parse_document(xml: &str) -> Result<Vec<XmlElement>, String> {
    use quick_xml::events::Event;

    let trimmed = xml.trim_start_matches('\u{feff}').trim();
    let mut reader = quick_xml::Reader::from_str(trimmed);

    let mut stack: Vec<XmlElement> = Vec::new();
    let mut roots: Vec<XmlElement> = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let mut elem = XmlElement {
                    name: local_name(e.name().as_ref()),
                    ..Default::default()
                };
                for a in e.attributes().flatten() {
                    elem.attrs.insert(
                        local_name(a.key.as_ref()),
                        a.unescape_value().unwrap_or_default().to_string(),
                    );
                }
                stack.push(elem);
            }
            Ok(Event::Empty(e)) => {
                let mut elem = XmlElement {
                    name: local_name(e.name().as_ref()),
                    ..Default::default()
                };
                for a in e.attributes().flatten() {
                    elem.attrs.insert(
                        local_name(a.key.as_ref()),
                        a.unescape_value().unwrap_or_default().to_string(),
                    );
                }
                append(&mut stack, &mut roots, elem);
            }
            Ok(Event::Text(t)) => {
                if let Some(top) = stack.last_mut() {
                    if let Ok(txt) = t.unescape() {
                        top.text.push_str(txt.trim());
                    }
                }
            }
            Ok(Event::End(_)) => {
                if let Some(mut finished) = stack.pop() {
                    // Trim trailing whitespace once on close.
                    finished.text = finished.text.trim().to_string();
                    append(&mut stack, &mut roots, finished);
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => return Err(format!("XML parse error: {e}")),
        }
    }

    if !stack.is_empty() {
        return Err("XML document is malformed (unclosed elements)".into());
    }
    Ok(roots)
}

fn append(stack: &mut [XmlElement], roots: &mut Vec<XmlElement>, elem: XmlElement) {
    match stack.last_mut() {
        Some(parent) => parent.children.push(elem),
        None => roots.push(elem),
    }
}

fn local_name(raw: &[u8]) -> String {
    std::str::from_utf8(raw)
        .unwrap_or("")
        .rsplit(':')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
}
