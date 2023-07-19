use color_eyre::{eyre::eyre, Result};
use std::fmt::Write;

#[derive(Debug)]
pub struct Template(Vec<Component>);

#[derive(Debug)]
enum Component {
    Literal(String),
    Author,
    Album,
    Track,
    Number,
    Extension,
}

pub struct TemplateFields<'a> {
    pub author: &'a str,
    pub track: &'a str,
    pub album: &'a str,
    pub extension: &'a str,
    pub seq: usize,
    pub seq_digits: usize,
}

impl Template {
    pub fn compile(template: &str) -> Result<Self> {
        let mut prev_pos = 0;
        let mut components: Vec<Component> = Vec::new();
        let template_bytes = template.as_bytes();
        for (pos, byte) in template_bytes.iter().enumerate() {
            if *byte == b'%' {
                let literal = &template[prev_pos..pos];
                if !literal.is_empty() {
                    components.push(Component::Literal(literal.to_string()));
                }
                prev_pos = pos + 2;
                match template_bytes.get(pos + 1) {
                    Some(b'a') => components.push(Component::Author),
                    Some(b't') => components.push(Component::Track),
                    Some(b'b') => components.push(Component::Album),
                    Some(b's') => components.push(Component::Number),
                    Some(b'e') => components.push(Component::Extension),
                    _ => return Err(eyre!("{template:?} is not a valid path template.")),
                }
            }
        }
        let remainder = &template[prev_pos..];
        if !remainder.is_empty() {
            components.push(Component::Literal(remainder.to_string()));
        }
        Ok(Self(components))
    }

    pub fn resolve(&self, fields: &TemplateFields) -> Result<String> {
        let mut output = String::new();
        for component in &self.0 {
            match component {
                Component::Literal(l) => output.push_str(l),
                Component::Author => output.push_str(fields.author),
                Component::Track => output.push_str(fields.track),
                Component::Album => output.push_str(fields.album),
                Component::Number => {
                    let (seq, seq_digits) = (fields.seq, fields.seq_digits);
                    write!(output, "{seq:0seq_digits$}")?;
                }
                Component::Extension => output.push_str(fields.extension),
            };
        }
        Ok(output)
    }
}
