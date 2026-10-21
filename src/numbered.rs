//! Numbers standing for things known by text while Glance runs (a note by its file's name, a program kept at hand by
//! what it opens): a press names what it lands on by one (a hit holds no text), not by its place among the others,
//! which changes as they do (a note written, a program moved) before the press is let go.

use std::collections::BTreeMap;
use std::sync::Mutex;

pub struct Numbered(Mutex<BTreeMap<String, u64>>);

impl Numbered {
    pub const fn new() -> Numbered {
        Numbered(Mutex::new(BTreeMap::new()))
    }

    /// The number standing for `name`: the same each time it is asked for, another's never.
    pub fn of(&self, name: &str) -> u64 {
        let mut numbers = self.0.lock().unwrap();
        if let Some(number) = numbers.get(name) {
            return *number;
        }
        let number = numbers.len() as u64;
        numbers.insert(name.to_string(), number);
        number
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_name_keeps_its_number() {
        let numbered = super::Numbered::new();
        let (a, b) = (numbered.of("a.md"), numbered.of("b.md"));
        assert_ne!(a, b);
        assert_eq!(numbered.of("a.md"), a);
        assert_eq!(numbered.of("b.md"), b);
    }
}
