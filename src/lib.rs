#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;

#[cfg(not(feature = "std"))]
use alloc::{vec, vec::Vec};

#[cfg(not(feature = "std"))]
use core as std;

use std::cell::RefCell;
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::ops::{Deref, DerefMut, Drop};
use std::rc::Rc;

/// A smart pointer for memory borrowed from a `MemoryBank<T>`. The data allocation will be preserved
/// even if the initiating `MemoryBank` is dropped.
pub struct Loan<T> {
    reference: Rc<T>,
    list_index: usize,
    parent_index_heap: Rc<RefCell<BinaryHeap<Reverse<usize>>>>,
}

impl<T> Deref for Loan<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { Rc::as_ptr(&self.reference).as_ref().unwrap() }
    }
}

impl<T> DerefMut for Loan<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { Rc::as_ptr(&self.reference).cast_mut().as_mut().unwrap() }
    }
}

impl<T> Drop for Loan<T> {
    fn drop(&mut self) {
        self.parent_index_heap
            .borrow_mut()
            .push(Reverse(self.list_index));
    }
}

/// A structure that reuses old data of type `T` to reduce the number of heap allocations.
pub struct MemoryBank<T> {
    list: Vec<Rc<T>>,
    available_indeces: Rc<RefCell<BinaryHeap<Reverse<usize>>>>,
}

impl<T: Default> Default for MemoryBank<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Default> MemoryBank<T> {
    /// Creates a new, empty `MemoryBank<T>`. The first loan is guaranteed to be a heap allocation.
    pub fn new() -> Self {
        Self {
            list: Vec::new(),
            available_indeces: Rc::new(RefCell::new(BinaryHeap::new())),
        }
    }

    /// Loans the user some memory to use. Will reuse an element from old memory if there are any, otherwise it will allocate on the heap.
    pub fn take_loan(&mut self) -> Loan<T> {
        let index = self
            .available_indeces
            .borrow_mut()
            .pop()
            .unwrap_or_else(|| {
                self.list.push(Rc::new(T::default()));
                Reverse(self.list.len() - 1)
            })
            .0;

        Loan {
            reference: self.list[index].clone(),
            list_index: index,
            parent_index_heap: self.available_indeces.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basics() {
        let mut bank: MemoryBank<Vec<i32>> = MemoryBank::new();

        let mut loan = bank.take_loan();
        loan.clone_from(&vec![1, 2, 3, 4, 5]);
        let loan1_capacity = loan.capacity();
        drop(loan);

        assert_eq!(bank.available_indeces.borrow().peek(), Some(&Reverse(0)));

        let loan2 = bank.take_loan();
        assert!(bank.available_indeces.borrow().is_empty());
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

        drop(loan2);
        assert_eq!(bank.available_indeces.borrow().peek(), Some(&Reverse(1)));

        drop(loan1);
        assert_eq!(bank.available_indeces.borrow().peek(), Some(&Reverse(0)));

        let loan3 = bank.take_loan();
        let loan4 = bank.take_loan();

        assert!(bank.available_indeces.borrow().is_empty());

        drop(bank);

        let mut loan1_vec = (0..100).collect::<Vec<i32>>();
        loan1_vec[5] = 2;
        assert_eq!(*loan3, loan1_vec);

        let mut loan2_vec = (0..500).collect::<Vec<i32>>();
        loan2_vec[3] = 5;
        assert_eq!(*loan4, loan2_vec);
    }
}
