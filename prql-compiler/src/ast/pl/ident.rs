use std::fmt::Write;

use itertools::Itertools;
use serde::{self, ser::SerializeSeq, Deserialize, Deserializer, Serialize, Serializer};

/// A name. Generally columns, tables, functions, variables.
/// This is glorified way of writing a "vec with at least one element".
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone)]
pub struct Ident {
    pub path: Vec<String>,
    pub name: String,
}

impl Ident {
    pub fn from_name<S: ToString>(name: S) -> Self {
        Ident {
            path: Vec::new(),
            name: name.to_string(),
        }
    }

    pub fn from_path<S: ToString>(mut path: Vec<S>) -> Self {
        let name = path.pop().unwrap().to_string();
        Ident {
            path: path.into_iter().map(|x| x.to_string()).collect(),
            name,
        }
    }

    pub fn pop(self) -> Option<Self> {
        let mut path = self.path;
        path.pop().map(|name| Ident { path, name })
    }

    pub fn pop_front(mut self) -> (String, Option<Ident>) {
        if self.path.is_empty() {
            (self.name, None)
        } else {
            let first = self.path.remove(0);
            (first, Some(self))
        }
    }

    pub fn with_name<S: ToString>(mut self, name: S) -> Self {
        self.name = name.to_string();
        self
    }

    pub fn starts_with(&self, prefix: &Ident) -> bool {
        if self.path.len() < prefix.path.len() {
            false
        } else {
            let self_chunks = self.path.iter().chain(Some(&self.name));
            let prefix_chunks = prefix.path.iter().chain(Some(&prefix.name));
            !std::iter::zip(self_chunks, prefix_chunks).all_equal()
        }
    }
}

impl std::fmt::Display for Ident {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        display_ident(f, self)
    }
}

impl IntoIterator for Ident {
    type Item = String;
    type IntoIter = std::iter::Chain<
        std::vec::IntoIter<std::string::String>,
        std::option::IntoIter<std::string::String>,
    >;

    fn into_iter(self) -> Self::IntoIter {
        self.path.into_iter().chain(Some(self.name))
    }
}

impl std::ops::Add<Ident> for Ident {
    type Output = Ident;

    fn add(self, rhs: Ident) -> Self::Output {
        Ident {
            path: self.into_iter().chain(rhs.path).collect(),
            name: rhs.name,
        }
    }
}

impl Serialize for Ident {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(self.path.len() + 1))?;
        for part in &self.path {
            seq.serialize_element(part)?;
        }
        seq.serialize_element(&self.name)?;
        seq.end()
    }
}

impl<'de> Deserialize<'de> for Ident {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        <Vec<String> as Deserialize>::deserialize(deserializer).map(Ident::from_path)
    }
}

pub fn display_ident(f: &mut std::fmt::Formatter, ident: &Ident) -> Result<(), std::fmt::Error> {
    for part in &ident.path {
        display_ident_part(f, part)?;
        f.write_char('.')?;
    }
    display_ident_part(f, &ident.name)?;
    Ok(())
}

pub fn display_ident_part(f: &mut std::fmt::Formatter, s: &str) -> Result<(), std::fmt::Error> {
    fn forbidden_start(c: char) -> bool {
        !(('a'..='z').contains(&c) || matches!(c, '_' | '$'))
    }
    fn forbidden_subsequent(c: char) -> bool {
        !(('a'..='z').contains(&c) || ('0'..='9').contains(&c) || matches!(c, '_'))
    }
    let needs_escape = s.is_empty()
        || s.starts_with(forbidden_start)
        || (s.len() > 1 && s.chars().skip(1).any(forbidden_subsequent));

    if needs_escape {
        write!(f, "`{s}`")
    } else {
        write!(f, "{s}")
    }
}
