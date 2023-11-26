#![doc = include_str!("../README.md")]
#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;

#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

#[cfg(not(feature = "std"))]
use alloc::boxed::Box;

#[cfg(not(feature = "std"))]
use alloc::rc::{Rc, Weak};
#[cfg(feature = "std")]
use std::rc::{Rc, Weak};

#[cfg(not(feature = "std"))]
use core as std;

use std::cell::RefCell;
use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::{Deref, DerefMut, Drop};
use std::ptr::NonNull;

/// A smart pointer for memory borrowed from a `MemoryBank<T>`. The data allocation will be preserved
/// even if the initiating `MemoryBank` is dropped.
pub struct Loan<T> {
    reference: NonNull<T>,
    parent_list: Weak<RefCell<Vec<NonNull<T>>>>,
}

impl<T> AsRef<T> for Loan<T> {
    fn as_ref(&self) -> &T {
        unsafe { self.reference.as_ref() }
    }
}

impl<T> AsMut<T> for Loan<T> {
    fn as_mut(&mut self) -> &mut T {
        unsafe { self.reference.as_mut() }
    }
}

impl<T> Deref for Loan<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { self.reference.as_ref() }
    }
}

impl<T> DerefMut for Loan<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { self.reference.as_mut() }
    }
}

impl<T> Drop for Loan<T> {
    fn drop(&mut self) {
        if let Some(parent_list) = self.parent_list.upgrade() {
            parent_list.borrow_mut().push(self.reference);
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for Loan<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl<T: fmt::Display> fmt::Display for Loan<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&**self, f)
    }
}

impl<T> fmt::Pointer for Loan<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Pointer::fmt(&self.reference, f)
    }
}

impl<T: Hash> Hash for Loan<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (**self).hash(state)
    }
}

impl<T: PartialEq> PartialEq for Loan<T> {
    fn eq(&self, other: &Self) -> bool {
        (**self).eq(&**other)
    }
}

impl<T: Eq> Eq for Loan<T> {}

impl<T: PartialOrd> PartialOrd for Loan<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        (**self).partial_cmp(&**other)
    }
}

impl<T: Ord> Ord for Loan<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        (**self).cmp(&**other)
    }
}

/// A structure that reuses old data of type `T` to reduce the number of heap allocations.
pub struct MemoryBank<T> {
    list: Rc<RefCell<Vec<NonNull<T>>>>,
}

impl<T> Default for MemoryBank<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Default> MemoryBank<T> {
    /// Loans the user some memory to use. Will reuse an element from old memory if there are any, otherwise it will allocate on the heap.
    ///
    /// If the loaned memory was previously used, it will be in the exact same state it was in
    /// right before the previous `Loan` got dropped.
    pub fn take_loan(&mut self) -> Loan<T> {
        let ptr = self
            .list
            .borrow_mut()
            .pop()
            .unwrap_or_else(|| Self::create_ptr(T::default()));

        Loan {
            reference: ptr,
            parent_list: Rc::downgrade(&self.list),
        }
    }
}

impl<T: Clone> MemoryBank<T> {
    /// Takes out a loan and clones its contents from `item`
    ///
    ///
    /// # Example
    /// ```
    /// use membank::MemoryBank;
    ///
    /// let mut bank = MemoryBank::new();
    ///
    /// let v = vec![1, 2, 3, 4, 5, 6];
    ///
    /// let loan = bank.take_loan_and_clone(&v);
    ///
    /// assert_eq!(v, *loan);
    /// ```
    pub fn take_loan_and_clone(&mut self, item: &T) -> Loan<T> {
        let ptr = match self.list.borrow_mut().pop() {
            Some(nn_ptr) => nn_ptr,
            None => Self::create_ptr(item.clone()),
        };

        Loan {
            reference: ptr,
            parent_list: Rc::downgrade(&self.list),
        }
    }
}

impl<T> MemoryBank<T> {
    #[inline]
    fn create_ptr(value: T) -> NonNull<T> {
        let bx = Box::new(value);

        NonNull::from(Box::leak(bx))
    }

    /// Creates a new, empty `MemoryBank<T>`. The first loan is guaranteed to be a heap allocation.
    pub fn new() -> Self {
        Self {
            list: Rc::new(RefCell::new(Vec::new())),
        }
    }

    /// Gives the bank ownership of `value` and returns it in a `Loan`.
    ///
    /// # Examples
    /// ```
    /// use membank::MemoryBank;
    ///
    /// let mut bank = MemoryBank::new();
    ///
    /// let v = vec![2.1, 4.3, 1.0, 0.98];
    /// let v_clone = v.clone();
    ///
    /// let loan = bank.deposit(v);
    ///
    /// assert_eq!(*loan, v_clone);
    /// ```
    pub fn deposit(&mut self, value: T) -> Loan<T> {
        let ptr = Self::create_ptr(value);

        Loan {
            reference: ptr,
            parent_list: Rc::downgrade(&self.list),
        }
    }

    /// Only take a loan if it's from previously used memory, if there is no previously allocated
    /// memory, returns `None`.
    ///
    ///
    /// # Example
    /// ```
    /// use membank::MemoryBank;
    ///
    /// let mut bank: MemoryBank<Vec<i32>> = MemoryBank::new();
    /// assert!(bank.take_old_loan().is_none());
    ///
    /// let loan1 = bank.take_loan();
    ///
    /// assert!(bank.take_old_loan().is_none());
    ///
    /// drop(loan1);
    /// assert!(bank.take_old_loan().is_some());
    /// ```
    pub fn take_old_loan(&mut self) -> Option<Loan<T>> {
        let ptr = self.list.borrow_mut().pop()?;

        Some(Loan {
            reference: ptr,
            parent_list: Rc::downgrade(&self.list),
        })
    }
}

impl<T> Drop for MemoryBank<T> {
    fn drop(&mut self) {
        let mut borrowed = self.list.borrow_mut();

        while let Some(nn_ptr) = borrowed.pop() {
            unsafe {
                let _bx = Box::from_raw(nn_ptr.as_ptr());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(feature = "std"))]
    use alloc::vec;

    #[cfg(feature = "std")]
    use std::collections::hash_map::DefaultHasher;

    #[test]
    fn basics() {
        let mut bank: MemoryBank<Vec<i32>> = MemoryBank::new();

        let mut loan = bank.take_loan();
        loan.clone_from(&vec![1, 2, 3, 4, 5]);
        let loan1_capacity = loan.capacity();
        drop(loan);

        assert_eq!(bank.list.borrow().len(), 1);

        let loan2 = bank.take_loan();
        assert!(bank.list.borrow().is_empty());
        assert_eq!(loan2.capacity(), loan1_capacity);
        assert_eq!(*loan2, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn multiloan() {
        let mut bank: MemoryBank<Vec<i32>> = MemoryBank::new();

        let mut loan1 = bank.take_loan();
        let mut loan2 = bank.take_loan();

        loan1.clone_from(&(0..100).collect());
        loan2.clone_from(&(0..500).collect());

        loan1[5] = 2;
        loan2[3] = 5;

        let loan1_vec = loan1.clone();
        let loan2_vec = loan2.clone();

        let loan2_ptr = loan2.reference.clone();
        drop(loan2);
        assert_eq!(loan2_ptr, bank.list.borrow()[0]);

        let loan1_ptr = loan1.reference.clone();
        drop(loan1);
        assert_eq!([loan2_ptr, loan1_ptr], **bank.list.borrow());

        let loan3 = bank.take_loan();
        let loan4 = bank.take_loan();

        assert!(bank.list.borrow().is_empty());

        drop(bank);

        assert_eq!(*loan3, loan1_vec);
        assert_eq!(*loan4, loan2_vec);
    }

    #[cfg(feature = "std")]
    #[test]
    fn hash_equality() {
        let mut bank: MemoryBank<String> = MemoryBank::new();

        let string = "Swimming with the fishes";

        let mut loan_string = bank.take_loan();
        loan_string.push_str(string);

        let mut string_hasher = DefaultHasher::new();
        string.hash(&mut string_hasher);

        let mut loan_hasher = DefaultHasher::new();
        loan_string.hash(&mut loan_hasher);

        assert_eq!(string_hasher.finish(), loan_hasher.finish());
    }

    #[test]
    fn comparisons() {
        let n1 = 40;
        let n2 = 30;

        let mut bank: MemoryBank<i32> = MemoryBank::new();

        let mut loan1 = bank.take_loan();
        *loan1 = n1;

        let mut loan2 = bank.take_loan();
        *loan2 = n2;

        assert!(loan1 > loan2);
        assert_ne!(loan1, loan2);
    }
}
