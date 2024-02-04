use color_eyre::{eyre::bail, Result};
use std::{borrow::Cow, fmt::Write};

#[derive(Debug)]
pub struct Template(Vec<Component>);

#[derive(Debug)]
enum Component {
    Literal(String),
    Artists,
    Title,
    Album,
    Seq,
    Track,
    Disc,
    Language,
    Year,
    Publisher,
}

pub struct TemplateFields<'a> {
    pub artists: Cow<'a, str>,
    pub title: Cow<'a, str>,
    pub album: Cow<'a, str>,
    pub seq: usize,
    pub seq_digits: usize,
    pub track: i32,
    pub disc: i32,
    pub language: Cow<'a, str>,
    pub year: i32,
    pub publisher: Cow<'a, str>,
}

impl<'a> TemplateFields<'a> {
    pub fn sanitize(&'a self) -> Self {
        TemplateFields {
            artists: sanitize_string(&self.artists),
            title: sanitize_string(&self.title),
            album: sanitize_string(&self.album),
            seq: self.seq,
            seq_digits: self.seq_digits,
            track: self.track,
            disc: self.disc,
            language: sanitize_string(&self.language),
            year: self.year,
            publisher: sanitize_string(&self.publisher),
        }
    }
}

#[cfg(unix)]
fn sanitize_pattern(c: char) -> bool {
    c == '/'
}

#[cfg(windows)]
fn sanitize_pattern(c: char) -> bool {
    c == '/'
        || c == '\\'
        || c == ':'
        || c == '*'
        || c == '?'
        || c == '"'
        || c == '<'
        || c == '>'
        || c == '|'
}

fn sanitize_string(string: &str) -> Cow<'_, str> {
    if string.contains(sanitize_pattern) {
        string.replace(sanitize_pattern, " ").into()
    } else {
        string.into()
    }
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
                    Some(b'a') => components.push(Component::Artists),
                    Some(b't') => components.push(Component::Title),
                    Some(b'b') => components.push(Component::Album),
                    Some(b's') => components.push(Component::Seq),
                    Some(b'n') => components.push(Component::Track),
                    Some(b'd') => components.push(Component::Disc),
                    Some(b'l') => components.push(Component::Language),
                    Some(b'y') => components.push(Component::Year),
                    Some(b'p') => components.push(Component::Publisher),
                    _ => bail!("{template:?} is not a valid path template."),
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
                Component::Artists => output.push_str(&fields.artists),
                Component::Title => output.push_str(&fields.title),
                Component::Album => output.push_str(&fields.album),
                Component::Seq => {
                    let (seq, seq_digits) = (fields.seq, fields.seq_digits);
                    write!(output, "{seq:0seq_digits$}")?;
                }
                Component::Track => write!(output, "{}", fields.track)?,
                Component::Disc => write!(output, "{}", fields.disc)?,
                Component::Language => output.push_str(&fields.language),
                Component::Year => write!(output, "{}", fields.year)?,
                Component::Publisher => output.push_str(&fields.publisher),
            };
        }
        Ok(output)
    }
}
