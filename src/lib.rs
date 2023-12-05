#![cfg_attr(feature = "std", doc = include_str!("../README.md"))]
#![cfg_attr(not(feature = "std"), no_std)]

use crossbeam_channel as cross;

#[cfg(not(feature = "std"))]
use core as std;

use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::mem::ManuallyDrop;
use std::ops::{Deref, DerefMut, Drop};
use std::time::Duration;

/// A smart pointer for memory borrowed from a `MemoryBank<T>`. The data allocation will be preserved
/// even if the initiating `MemoryBank` is dropped.
pub struct Loan<T> {
    reference: ManuallyDrop<T>,
    tx: cross::Sender<T>,
}

impl<T> Loan<T> {
    fn new(value: T, tx: cross::Sender<T>) -> Self {
        Self {
            reference: ManuallyDrop::new(value),
            tx,
        }
    }
}

impl<T> AsRef<T> for Loan<T> {
    fn as_ref(&self) -> &T {
        &self.reference
    }
}

impl<T> AsMut<T> for Loan<T> {
    fn as_mut(&mut self) -> &mut T {
        &mut self.reference
    }
}

impl<T> Deref for Loan<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.reference
    }
}

impl<T> DerefMut for Loan<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.reference
    }
}

impl<T> Drop for Loan<T> {
    fn drop(&mut self) {
        unsafe {
            let _ = self.tx.send(ManuallyDrop::take(&mut self.reference));
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for Loan<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self, f)
    }
}

impl<T: fmt::Display> fmt::Display for Loan<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self, f)
    }
}

impl<T: Hash> Hash for Loan<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (**self).hash(state)
    }
}

impl<T: PartialEq> PartialEq for Loan<T> {
    fn eq(&self, other: &Self) -> bool {
        (**self).eq(other)
    }
}

impl<T: Eq> Eq for Loan<T> {}

impl<T: PartialOrd> PartialOrd for Loan<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        (**self).partial_cmp(other)
    }
}

impl<T: Ord> Ord for Loan<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        (**self).cmp(other)
    }
}

/// A structure that reuses old data of type `T` to reduce the number of new allocations (useful for heap allocations).
#[derive(Clone)]
pub struct MemoryBank<T> {
    rx: cross::Receiver<T>,
    tx: cross::Sender<T>,
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
    /// right before the previous [`Loan`] got dropped.
    pub fn take_loan(&self) -> Loan<T> {
        let value = self.rx.try_recv().unwrap_or_default();

        Loan::new(value, self.tx.clone())
    }
}

impl<T> MemoryBank<T> {
    /// Creates a new, empty `MemoryBank<T>`. The first loan is guaranteed to be a heap allocation.
    pub fn new() -> Self {
        let (tx, rx) = cross::unbounded();
        Self { tx, rx }
    }

    /// Gives the bank ownership of `value` and returns it in a [`Loan`].
    ///
    /// # Examples
    /// ```
    /// use membank::MemoryBank;
    ///
    /// let bank = MemoryBank::new();
    ///
    /// let v = vec![2.1, 4.3, 1.0, 0.98];
    /// let v_clone = v.clone();
    ///
    /// let loan = bank.deposit(v);
    ///
    /// assert_eq!(*loan, v_clone);
    /// ```
    pub fn deposit(&self, value: T) -> Loan<T> {
        Loan::new(value, self.tx.clone())
    }

    /// Only take a loan if it's from previously used memory, if there is no previously allocated
    /// memory, returns `None`.
    ///
    ///
    /// # Example
    /// ```
    /// use membank::MemoryBank;
    ///
    /// let bank: MemoryBank<Vec<i32>> = MemoryBank::new();
    /// assert!(bank.take_old_loan().is_none());
    ///
    /// let loan1 = bank.take_loan();
    ///
    /// assert!(bank.take_old_loan().is_none());
    ///
    /// drop(loan1);
    /// assert!(bank.take_old_loan().is_some());
    /// ```
    pub fn take_old_loan(&self) -> Option<Loan<T>> {
        Some(Loan::new(self.rx.try_recv().ok()?, self.tx.clone()))
    }

    /// Only take previously used memory. Will return `None` if it waits past `timeout`.
    ///
    /// # Example
    /// ```
    /// use membank::MemoryBank;
    /// use std::time::Duration;
    ///
    /// let bank: MemoryBank<Vec<f64>> = MemoryBank::new();
    /// assert!(bank.take_old_loan_timeout(Duration::from_millis(100)).is_none());
    ///
    /// let mut loan1 = bank.take_loan();
    /// loan1.push(3.21);
    /// drop(loan1);
    ///
    /// let loan2 = bank.take_old_loan_timeout(Duration::from_millis(100)).unwrap();
    /// assert_eq!(*loan2, vec![3.21]);
    /// ```
    pub fn take_old_loan_timeout(&self, timeout: Duration) -> Option<Loan<T>> {
        Some(Loan::new(
            self.rx.recv_timeout(timeout).ok()?,
            self.tx.clone(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(feature = "std"))]
    extern crate alloc;

    #[cfg(not(feature = "std"))]
    use alloc::{vec, vec::Vec};

    #[cfg(feature = "std")]
    use std::collections::hash_map::DefaultHasher;

    #[test]
    fn basics() {
        let bank: MemoryBank<Vec<i32>> = MemoryBank::new();

        let v = vec![1, 2, 3, 4, 5];

        let mut loan = bank.take_loan();
        loan.clone_from(&v);
        drop(loan);

        let loan2 = bank.take_old_loan().unwrap();
        assert!(bank.take_old_loan().is_none());
        assert_eq!(*loan2, v);
    }

    #[test]
    fn multiloan() {
        let bank: MemoryBank<Vec<i32>> = MemoryBank::new();

        let mut loan1 = bank.take_loan();
        let mut loan2 = bank.take_loan();

        loan1.clone_from(&(0..100).collect());
        loan2.clone_from(&(0..500).collect());

        loan1[5] = 2;
        loan2[3] = 5;

        let loan1_vec = loan1.clone();
        let loan2_vec = loan2.clone();

        drop(loan2);
        drop(loan1);

        let loan3 = bank.take_old_loan().unwrap();
        let loan4 = bank.take_old_loan().unwrap();

        assert!(bank.take_old_loan().is_none());

        drop(bank);

        assert_eq!(*loan3, loan2_vec);
        assert_eq!(*loan4, loan1_vec);
    }

    #[cfg(feature = "std")]
    #[test]
    fn hash_equality() {
        let bank: MemoryBank<String> = MemoryBank::new();

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

        let bank: MemoryBank<i32> = MemoryBank::new();

        let mut loan1 = bank.take_loan();
        *loan1 = n1;

        let mut loan2 = bank.take_loan();
        *loan2 = n2;

        assert!(loan1 > loan2);
        assert_ne!(loan1, loan2);
    }

    #[cfg(feature = "std")]
    #[test]
    fn bank_between_threads() {
        let bank: MemoryBank<Vec<i32>> = MemoryBank::default();

        let mut v1 = vec![0, 2, 3, 5];

        let mut loan = bank.take_loan();
        loan.clone_from(&v1);
        loan[0] = 1;
        v1[0] = 1;

        assert_eq!(*loan, v1);

        drop(loan);

        let bank_clone = bank.clone();
        let v1_clone = v1.clone();

        let thread = std::thread::spawn(move || {
            let mut loan = bank_clone.take_old_loan().unwrap();

            assert_eq!(*loan, v1_clone);

            loan[1] = 1;
        });

        v1[1] = 1;

        let _ = thread.join();
        let loan = bank.take_loan();
        assert_eq!(*loan, v1);
    }

    #[cfg(feature = "std")]
    #[test]
    fn loan_between_threads() {
        let bank: MemoryBank<Vec<i32>> = MemoryBank::default();

        let mut v = vec![17, 23, 1, 4, 5];

        let mut loan = bank.take_loan();
        loan.clone_from(&v);
        loan[0] = 3;
        v[0] = 3;

        let v_clone = v.clone();
        let thread = std::thread::spawn(move || {
            assert_eq!(*loan, v_clone);

            loan[2] = 7;
        });

        v[2] = 7;
        let _ = thread.join();
        let loan = bank.take_loan();
        assert_eq!(*loan, v);
    }
}
