use stable_deref_trait::StableDeref;
use std::borrow::Borrow;
use std::cmp::Ordering;
use std::hash::{Hash, Hasher};
use std::ops::Deref;

/// Wrapper type whose "identity" for equality and hashing is determined from the reference itself
/// (ie. the pointer) and not from the underlying data.
#[derive(Debug)]
pub struct RefId<'a, T: ?Sized>(pub &'a T);

impl<'a, T> Clone for RefId<'a, T> {
    fn clone(&self) -> Self {
        RefId(self.0)
    }
}

impl<'a, T> Copy for RefId<'a, T> {}

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

impl<'a, 'b, T> PartialOrd<RefId<'b, T>> for RefId<'a, T> {
    fn partial_cmp(&self, other: &RefId<'b, T>) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<'a, T> Ord for RefId<'a, T> {
    fn cmp(&self, other: &RefId<'a, T>) -> Ordering {
        (self.0 as *const T).cmp(&(other.0 as *const T))
    }
}

impl<'a, T: ?Sized> Deref for RefId<'a, T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<'a, T: ?Sized> Borrow<T> for RefId<'a, T> {
    fn borrow(&self) -> &T {
        &*self.0
    }
}
/*
impl<'a, T: ?Sized, U: ?Sized> AsRef<U> for RefId<'a, T>
where
    T: AsRef<U>,
{
    fn as_ref(&self) -> &U {
        <T as AsRef<U>>::as_ref(&*self)
    }
}
*/
unsafe impl<'a, T: ?Sized> StableDeref for RefId<'a, T> {}
