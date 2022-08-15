use std::hash::{Hash, Hasher};

/// Wrapper type whose "identity" for equality and hashing is determined from the reference itself
/// (ie. the pointer) and not from the underlying data.
#[derive(Debug)]
pub struct RefId<'a, T>(pub &'a T);

impl<'a, T> Hash for RefId<'a, T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::ptr::hash(self.0, state)
    }
}

impl<'a, 'b, T> PartialEq<RefId<'b, T>> for RefId<'a, T> {
    fn eq(&self, other: &RefId<'b, T>) -> bool {
        std::ptr::eq(self.0, other.0)
    }
}

impl<'a, T> Eq for RefId<'a, T> {}

