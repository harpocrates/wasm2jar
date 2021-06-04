use std::collections::HashSet;

pub trait Renamer {
    /// Rename a function's unqualified name
    fn rename_function(&mut self, name: &str) -> String;

    /// Rename a table's unqualified name
    fn rename_table(&mut self, name: &str) -> String;

    /// Rename a global's unqualified name
    fn rename_global(&mut self, name: &str) -> String;
}

/// Doesn't rename anything
pub struct IdentityRenamer;

impl Renamer for IdentityRenamer {
    fn rename_function(&mut self, name: &str) -> String {
        name.to_owned()
    }

    fn rename_table(&mut self, name: &str) -> String {
        name.to_owned()
    }

    fn rename_global(&mut self, name: &str) -> String {
        name.to_owned()
    }
}

/// Renames into something that is callable from Java
pub struct JavaRenamer(HashSet<String>);

impl JavaRenamer {
    pub fn new() -> JavaRenamer {
        JavaRenamer(
            Self::RESERVED_IDENTIFIERS
                .iter()
                .copied()
                .map(String::from)
                .collect(),
        )
    }

    pub const RESERVED_IDENTIFIERS: [&'static str; 58] = [
        "abstract",
        "continue",
        "for",
        "new",
        "switch",
        "assert",
        "default",
        "if",
        "package",
        "synchronized",
        "boolean",
        "do",
        "goto",
        "private",
        "this",
        "break",
        "double",
        "implements",
        "protected",
        "throw",
        "byte",
        "else",
        "import",
        "public",
        "throws",
        "case",
        "enum",
        "instanceof",
        "return",
        "transient",
        "catch",
        "extends",
        "int",
        "short",
        "try",
        "char",
        "final",
        "interface",
        "static",
        "void",
        "class",
        "finally",
        "long",
        "strictfp",
        "volatile",
        "const",
        "float",
        "native",
        "super",
        "while",
        "_",
        "true",
        "false",
        "null",
        "var",
        "yield",
        "record",
        "sealed",
    ];

    fn rename(&mut self, name: &str) -> String {
        let mut new_name = String::new();
        for c in name.to_owned().chars() {
            match c {
                c @ 'a'..='z' => new_name.push(c),
                c @ 'A'..='Z' => new_name.push(c),
                c @ '0'..='9' => {
                    if new_name.is_empty() {
                        new_name.push('_');
                    }
                    new_name.push(c);
                }
                _ => new_name.push('_'),
            }
        }

        while self.0.contains(&new_name) {
            new_name.push('_');
        }

        new_name
    }
}

impl Renamer for JavaRenamer {
    fn rename_function(&mut self, name: &str) -> String {
        self.rename(name)
    }

    fn rename_table(&mut self, name: &str) -> String {
        self.rename(name)
    }

    fn rename_global(&mut self, name: &str) -> String {
        self.rename(name)
    }
}
