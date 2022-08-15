use std::fmt::{Debug, Error, Formatter};
use std::iter::{DoubleEndedIterator, Enumerate, Extend, FromIterator};
use std::ops::Sub;
use std::result::Result;
use std::slice::Iter;
use std::vec::IntoIter as VecIntoIter;

/// Elements with a width (eg. when used in an `OffsetVec`)
pub trait Width {
    fn width(&self) -> usize;
}

/// A vector of elements of different logical "widths", where offsets into the vector are given in
/// terms of the sum of the widths of the previous elements (as opposed to the number of preceding
/// elements).
///
/// This sort of structure ends up being convenient in several places for modelling JVM classfiles:
///
///   - constant pool and indices (most entries have width 1, but some have width 2)
///   - method code and jump targets (different instructions have different sizes)
///   - local variables (depending on type, they have width 1 or 2)
///   - frames (again dependending on type, they have width 1 or 2)
///
#[derive(Clone)]
pub struct OffsetVec<T: Sized> {
    /// Entries, along with their offset
    entries: Vec<(Offset, T)>,

    /// Offset of the next element to be added
    offset_len: Offset,

    /// Offset for the first element (usually 0, but sometimes 1)
    initial_offset: Offset,
}

/// Offset into an `OffsetVec`
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct Offset(pub usize);

impl Sub for Offset {
    type Output = isize;

    fn sub(self, other: Offset) -> isize {
        (self.0 as isize) - (other.0 as isize)
    }
}

impl<T: Sized + Width> OffsetVec<T> {
    /// New empty offset vector
    pub fn new() -> OffsetVec<T> {
        OffsetVec {
            entries: vec![],
            offset_len: Offset(0),
            initial_offset: Offset(0),
        }
    }

    /// New empty offset vector, with a custom starting offset
    pub fn new_starting_at(initial_offset: Offset) -> OffsetVec<T> {
        OffsetVec {
            entries: vec![],
            offset_len: initial_offset,
            initial_offset,
        }
    }

    /// Length of the `OffsetVec` (aka. number of entries)
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Current offset size of the `OffsetVec` (aka. offset of the next element
    /// to be added)
    pub fn offset_len(&self) -> Offset {
        self.offset_len
    }

    /// Add an entry to the back
    pub fn push(&mut self, slot: T) -> Offset {
        let offset = self.offset_len;
        self.offset_len.0 += slot.width();
        self.entries.push((offset, slot));

        offset
    }

    /// Remove an entry from the back
    pub fn pop(&mut self) -> Option<(Offset, usize, T)> {
        self.entries.pop().map(|(off, elem)| {
            self.offset_len = off;
            (off, self.entries.len(), elem)
        })
    }

    /// Empty the vector
    pub fn clear(&mut self) {
        self.entries.clear();
        self.offset_len = self.initial_offset;
    }

    /// Get an entry (and its index) by its offset in the vector
    ///
    /// Note: this uses binary search to find the offset
    pub fn get_offset(&self, offset: Offset) -> OffsetResult<T> {
        match self.entries.binary_search_by_key(&offset, |(off, _)| *off) {
            Err(insert_at) if insert_at == self.entries.len() => OffsetResult::TooLarge,
            Err(insert_at) => OffsetResult::InvalidOffset(insert_at),
            Ok(found_idx) => OffsetResult::Ok(found_idx, &self.entries[found_idx].1),
        }
    }

    /// Set an entry by its offset in the vector
    ///
    /// Note: this uses binary search to find the offset
    pub fn set_offset(&mut self, offset: Offset, value: T) -> OffsetResult<'static, ()> {
        if offset == self.offset_len() {
            self.push(value);
            OffsetResult::Ok(self.len() - 1, &())
        } else {
            match self.entries.binary_search_by_key(&offset, |(off, _)| *off) {
                Err(insert_at) if insert_at == self.entries.len() => OffsetResult::TooLarge,
                Err(insert_at) => OffsetResult::InvalidOffset(insert_at),
                Ok(found_idx) => {
                    let replacing = &mut self.entries[found_idx].1;
                    if replacing.width() != value.width() {
                        OffsetResult::IncompatibleWidth(value.width(), replacing.width())
                    } else {
                        let mut value = value;
                        std::mem::swap(replacing, &mut value);
                        OffsetResult::Ok(found_idx, &())
                    }
                }
            }
        }
    }

    /// Get an entry (and its offset) by its position in the vector
    pub fn get_index(&self, index: usize) -> Option<(Offset, &T)> {
        self.entries.get(index).map(|(offset, t)| (*offset, t))
    }

    pub fn iter<'a>(&'a self) -> OffsetVecIter<'a, T> {
        self.into_iter()
    }
}

impl<A: PartialEq> PartialEq for OffsetVec<A> {
    fn eq(&self, other: &Self) -> bool {
        self.entries == other.entries
    }
}

impl<A: Eq> Eq for OffsetVec<A> {}

impl<A: Width> Default for OffsetVec<A> {
    fn default() -> Self {
        OffsetVec::new()
    }
}

pub enum OffsetResult<'a, T> {
    /// Element was accessed
    Ok(usize, &'a T),

    /// Offset was invalid, and falls in the middle of the element at this index
    InvalidOffset(usize),

    /// Width is incompatible (only occurs when trying to set an element)
    IncompatibleWidth(usize, usize),

    /// Offset is too big
    TooLarge,
}

impl<'a, T> OffsetResult<'a, T> {
    /// Convert to an `Option` and keep only the value found
    pub fn ok(&self) -> Option<&'a T> {
        match self {
            OffsetResult::Ok(_, found) => Some(found),
            OffsetResult::InvalidOffset(_)
            | OffsetResult::TooLarge
            | OffsetResult::IncompatibleWidth(_, _) => None,
        }
    }
}

/// Iterator for owned `OffsetVec`
pub struct OffsetVecIntoIter<T>(Enumerate<VecIntoIter<(Offset, T)>>);

impl<T> Iterator for OffsetVecIntoIter<T> {
    type Item = (Offset, usize, T);

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|(idx, (off, elem))| (off, idx, elem))
    }
}

impl<T> DoubleEndedIterator for OffsetVecIntoIter<T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.0
            .next_back()
            .map(|(idx, (off, elem))| (off, idx, elem))
    }
}

impl<T> IntoIterator for OffsetVec<T> {
    type Item = (Offset, usize, T);
    type IntoIter = OffsetVecIntoIter<T>;

    fn into_iter(self) -> OffsetVecIntoIter<T> {
        OffsetVecIntoIter(self.entries.into_iter().enumerate())
    }
}

/// Iterator for borrowed `OffsetVec`
pub struct OffsetVecIter<'a, T>(Enumerate<Iter<'a, (Offset, T)>>);

impl<'a, T> Iterator for OffsetVecIter<'a, T> {
    type Item = (Offset, usize, &'a T);

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|(idx, (off, elem))| (*off, idx, elem))
    }
}

impl<'a, T> DoubleEndedIterator for OffsetVecIter<'a, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.0
            .next_back()
            .map(|(idx, (off, elem))| (*off, idx, elem))
    }
}

impl<'a, T> IntoIterator for &'a OffsetVec<T> {
    type Item = (Offset, usize, &'a T);
    type IntoIter = OffsetVecIter<'a, T>;

    fn into_iter(self) -> OffsetVecIter<'a, T> {
        OffsetVecIter(self.entries.iter().enumerate())
    }
}

impl<T: Width> FromIterator<T> for OffsetVec<T> {
    fn from_iter<A: IntoIterator<Item = T>>(elems: A) -> Self {
        let mut offset_vec = OffsetVec::new();
        for elem in elems {
            offset_vec.push(elem);
        }
        offset_vec
    }
}

impl<T: Width> Extend<T> for OffsetVec<T> {
    fn extend<U: IntoIterator<Item = T>>(&mut self, iter: U) {
        for elem in iter {
            self.push(elem);
        }
    }
}

impl<T: Debug> Debug for OffsetVec<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        let mut list = f.debug_list();
        for (off, elem) in &self.entries {
            list.entry(&format_args!("#{} = {:?}", off.0, elem));
        }
        list.finish()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[derive(Copy, Clone, Eq, PartialEq, Debug)]
    enum Slot {
        OneWide(u8),
        TwoWide(u8),
        ThreeWide(u8),
    }

    impl Width for Slot {
        fn width(&self) -> usize {
            match self {
                Slot::OneWide(_) => 1,
                Slot::TwoWide(_) => 2,
                Slot::ThreeWide(_) => 3,
            }
        }
    }

    #[test]
    fn stack_of_same_sized_slots() {
        let slots: OffsetVec<Slot> = vec![Slot::OneWide(1), Slot::OneWide(2), Slot::OneWide(3)]
            .into_iter()
            .collect();
        assert_eq!(
            slots.into_iter().collect::<Vec<_>>(),
            vec![
                (Offset(0), 0, Slot::OneWide(1)),
                (Offset(1), 1, Slot::OneWide(2)),
                (Offset(2), 2, Slot::OneWide(3)),
            ]
        );

        let slots: OffsetVec<Slot> = vec![Slot::TwoWide(1), Slot::TwoWide(2), Slot::TwoWide(3)]
            .into_iter()
            .collect();
        assert_eq!(
            slots.into_iter().collect::<Vec<_>>(),
            vec![
                (Offset(0), 0, Slot::TwoWide(1)),
                (Offset(2), 1, Slot::TwoWide(2)),
                (Offset(4), 2, Slot::TwoWide(3)),
            ]
        );
    }

    #[test]
    fn stack_of_differently_sized_slots() {
        let slots: OffsetVec<Slot> = vec![
            Slot::OneWide(1),
            Slot::ThreeWide(2),
            Slot::TwoWide(3),
            Slot::TwoWide(4),
            Slot::OneWide(5),
            Slot::ThreeWide(6),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            slots.into_iter().collect::<Vec<_>>(),
            vec![
                (Offset(0), 0, Slot::OneWide(1)),
                (Offset(1), 1, Slot::ThreeWide(2)),
                (Offset(4), 2, Slot::TwoWide(3)),
                (Offset(6), 3, Slot::TwoWide(4)),
                (Offset(8), 4, Slot::OneWide(5)),
                (Offset(9), 5, Slot::ThreeWide(6)),
            ]
        );
    }
}
