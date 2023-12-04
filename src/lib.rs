#![doc = include_str!("../README.md")]
#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
use core as std;

use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::{Deref, DerefMut, Drop};

pub mod unsync {
    use super::*;

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

    use std::cell::RefCell;
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
        /// right before the previous [`Loan`] got dropped.
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
        /// use membank::unsync::MemoryBank;
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

        /// Gives the bank ownership of `value` and returns it in a [`Loan`].
        ///
        /// # Examples
        /// ```
        /// use membank::unsync::MemoryBank;
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
        /// use membank::unsync::MemoryBank;
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
}

#[cfg(feature = "std")]
pub mod sync {
    use super::*;
    use std::marker::{PhantomData, Send, Sync};
    use std::mem::ManuallyDrop;
    use std::sync::mpsc::{channel, Sender};
    use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

    pub struct BlockingDrop;
    pub struct NonBlockingDrop;

    pub type LockResult<'a, R, C> = Result<R, PoisonError<MutexGuard<'a, Vec<C>>>>;

    /// Thread-safe version of [`Loan`](crate::unsync::Loan).
    pub struct Loan<State, T> {
        reference: ManuallyDrop<T>,
        tx: Option<Sender<T>>,
        parent_list: Option<Weak<Mutex<Vec<T>>>>,
        state: PhantomData<State>,
    }

    impl<T> Loan<NonBlockingDrop, T> {
        fn new_nonblocking(value: T, tx: Sender<T>) -> Self {
            Self {
                reference: ManuallyDrop::new(value),
                tx: Some(tx),
                parent_list: None,
                state: PhantomData::<NonBlockingDrop>,
            }
        }
    }

    impl<T> Loan<BlockingDrop, T> {
        fn new_blocking(value: T, parent_list: Weak<Mutex<Vec<T>>>) -> Self {
            Self {
                reference: ManuallyDrop::new(value),
                tx: None,
                parent_list: Some(parent_list),
                state: PhantomData::<BlockingDrop>,
            }
        }
    }

    impl<State, T> AsRef<T> for Loan<State, T> {
        fn as_ref(&self) -> &T {
            &self.reference
        }
    }

    impl<State, T> AsMut<T> for Loan<State, T> {
        fn as_mut(&mut self) -> &mut T {
            &mut self.reference
        }
    }

    impl<State, T> Deref for Loan<State, T> {
        type Target = T;

        fn deref(&self) -> &Self::Target {
            &self.reference
        }
    }

    impl<State, T> DerefMut for Loan<State, T> {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.reference
        }
    }

    impl<State, T> Drop for Loan<State, T> {
        fn drop(&mut self) {
            unsafe {
                if let Some(tx) = self.tx.as_ref() {
                    let _ = tx.send(ManuallyDrop::take(&mut self.reference));
                } else if let Some(parent_list) = self.parent_list.as_ref() {
                    if let Some(parent_list_mutex) = parent_list.upgrade() {
                        if let Ok(mut parent_list) = parent_list_mutex.lock() {
                            parent_list.push(ManuallyDrop::take(&mut self.reference));
                        }
                    }
                }
            }
        }
    }

    impl<State, T: fmt::Debug> fmt::Debug for Loan<State, T> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            fmt::Debug::fmt(&self, f)
        }
    }

    impl<State, T: fmt::Display> fmt::Display for Loan<State, T> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            fmt::Display::fmt(&self, f)
        }
    }

    impl<State, T: Hash> Hash for Loan<State, T> {
        fn hash<H: Hasher>(&self, state: &mut H) {
            self.as_ref().hash(state)
        }
    }

    impl<State, T: PartialEq> PartialEq for Loan<State, T> {
        fn eq(&self, other: &Self) -> bool {
            self.as_ref().eq(other)
        }
    }

    impl<State, T: Eq> Eq for Loan<State, T> {}

    impl<State, T: PartialOrd> PartialOrd for Loan<State, T> {
        fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
            self.as_ref().partial_cmp(other)
        }
    }

    impl<State, T: Ord> Ord for Loan<State, T> {
        fn cmp(&self, other: &Self) -> Ordering {
            self.as_ref().cmp(other)
        }
    }

    /// Thread-safe version of [`MemoryBank`](crate::unsync::MemoryBank).
    ///
    /// Has interior mutability, so the user only needs to wrap it in an [Arc] to transfer it between threads.
    ///
    /// # Example
    /// ```
    /// use membank::sync::{BlockingDrop, MemoryBank};
    /// use std::sync::Arc;
    ///
    /// let bank: Arc<MemoryBank<BlockingDrop, _>> = Arc::new(MemoryBank::default());
    ///
    /// let v = vec![0, 4, 5];
    ///
    /// let loan1 = bank.take_loan_and_clone(&v).unwrap();
    ///
    /// let bank_clone = Arc::clone(&bank);
    /// std::thread::spawn(move || {
    ///     let loan2 = bank_clone.take_loan_and_clone(&v).unwrap();
    ///     assert_eq!(loan1, loan2);
    /// });
    /// ```
    pub struct MemoryBank<State, T> {
        list: Arc<Mutex<Vec<T>>>,
        tx: Option<Sender<T>>,
        state: PhantomData<State>,
    }

    impl<T: Send + Sync + 'static> Default for MemoryBank<NonBlockingDrop, T> {
        fn default() -> Self {
            let (tx, rx) = channel::<T>();

            let list = Arc::new(Mutex::new(Vec::new()));

            let list_clone = Arc::clone(&list);
            std::thread::spawn(move || {
                while let Ok(allocation) = rx.recv() {
                    if let Ok(mut guard) = list_clone.lock() {
                        guard.push(allocation);
                    }
                }
            });

            Self {
                list,
                tx: Some(tx),
                state: PhantomData::<NonBlockingDrop>,
            }
        }
    }

    impl<T: Send + Sync + 'static> MemoryBank<NonBlockingDrop, T> {
        pub fn new_nonblocking() -> Self {
            Self::default()
        }

        #[cfg(test)]
        fn new_nonblocking_with_notifier(notifier: Sender<()>) -> Self {
            let (tx, rx) = channel::<T>();

            let list = Arc::new(Mutex::new(Vec::new()));

            let list_clone = Arc::clone(&list);
            std::thread::spawn(move || {
                while let Ok(allocation) = rx.recv() {
                    if let Ok(mut guard) = list_clone.lock() {
                        guard.push(allocation);
                    }

                    let _ = notifier.send(());
                }
            });

            Self {
                list,
                tx: Some(tx),
                state: PhantomData::<NonBlockingDrop>,
            }
        }
    }

    impl<T> Default for MemoryBank<BlockingDrop, T> {
        fn default() -> Self {
            Self {
                list: Arc::new(Mutex::new(Vec::new())),
                tx: None,
                state: PhantomData::<BlockingDrop>,
            }
        }
    }

    impl<T: Default> MemoryBank<NonBlockingDrop, T> {
        /// Loans the user some memory to use. Will reuse an element from old memory if there are any, otherwise it will allocate on the heap.
        ///
        /// Will wait if the bank's internal mutex is locked by another thread.
        ///
        /// # Errors
        /// If another user of the bank panicked while holding it, an error will be returned.
        ///
        /// If the loaned memory was previously used, it will be in the exact same state it was in
        /// right before the previous [`Loan`] got dropped.
        pub fn take_loan(&self) -> LockResult<Loan<NonBlockingDrop, T>, T> {
            let value = self.list.lock()?.pop().unwrap_or_default();

            Ok(Loan::new_nonblocking(
                value,
                self.tx.as_ref().unwrap().clone(),
            ))
        }

        /// Loans out a new empty allocation. Will not block the thread.
        pub fn take_new_loan(&self) -> Loan<NonBlockingDrop, T> {
            Loan::new_nonblocking(T::default(), self.tx.as_ref().unwrap().clone())
        }
    }

    impl<T: Default> MemoryBank<BlockingDrop, T> {
        /// Loans the user some memory to use. Will reuse an element from old memory if there are any, otherwise it will allocate on the heap.
        ///
        /// Will wait if the bank's internal mutex is locked by another thread.
        ///
        /// # Errors
        /// If another user of the bank panicked while holding it, an error will be returned.
        ///
        /// If the loaned memory was previously used, it will be in the exact same state it was in
        /// right before the previous [`Loan`] got dropped.
        pub fn take_loan(&self) -> LockResult<Loan<BlockingDrop, T>, T> {
            let value = self.list.lock()?.pop().unwrap_or_default();

            Ok(Loan::new_blocking(value, Arc::downgrade(&self.list)))
        }

        /// Loans out a new empty allocation. Never has to wait for a mutex lock.
        pub fn take_new_loan(&self) -> Loan<BlockingDrop, T> {
            Loan::new_blocking(T::default(), Arc::downgrade(&self.list))
        }
    }

    impl<T: Clone> MemoryBank<NonBlockingDrop, T> {
        pub fn take_loan_and_clone(&self, item: &T) -> LockResult<Loan<NonBlockingDrop, T>, T> {
            let value = self.list.lock()?.pop().unwrap_or_else(|| item.clone());

            Ok(Loan::new_nonblocking(
                value,
                self.tx.as_ref().unwrap().clone(),
            ))
        }
    }

    impl<T: Clone> MemoryBank<BlockingDrop, T> {
        /// Takes out a loan and clones its contents from `item`.
        ///
        /// Will wait if the bank's internal mutex is locked by another thread.
        ///
        /// # Errors
        /// If another user of the bank panicked while holding it, an error will be returned.
        ///
        ///
        /// # Example
        /// ```
        /// use membank::sync::{BlockingDrop, MemoryBank};
        ///
        /// let bank: MemoryBank<BlockingDrop, _> = MemoryBank::default();
        ///
        /// let v = vec![1, 2, 3, 4, 5, 6];
        ///
        /// let loan = bank.take_loan_and_clone(&v).unwrap();
        ///
        /// assert_eq!(v, *loan);
        /// ```
        pub fn take_loan_and_clone(&self, item: &T) -> LockResult<Loan<BlockingDrop, T>, T> {
            let ptr = self.list.lock()?.pop().unwrap_or_else(|| item.clone());

            Ok(Loan::new_blocking(ptr, Arc::downgrade(&self.list)))
        }
    }

    impl<T> MemoryBank<NonBlockingDrop, T> {
        /// Gives the bank ownership of `value` and returns it in a [`Loan`].
        ///
        /// Will wait if the bank's internal mutex is locked by another thread.
        pub fn deposit(&self, value: T) -> Loan<NonBlockingDrop, T> {
            Loan::new_nonblocking(value, self.tx.as_ref().unwrap().clone())
        }

        pub fn take_old_loan(&self) -> LockResult<Option<Loan<NonBlockingDrop, T>>, T> {
            // TODO: better return type?
            match self.list.lock()?.pop() {
                Some(value) => Ok(Some(Loan::new_nonblocking(
                    value,
                    self.tx.as_ref().unwrap().clone(),
                ))),
                None => Ok(None),
            }
        }
    }

    impl<T> MemoryBank<BlockingDrop, T> {
        pub fn new_blocking() -> Self {
            Self::default()
        }

        /// Gives the bank ownership of `value` and returns it in a [`Loan`].
        ///
        /// Will wait if the bank's internal mutex is locked by another thread.
        ///
        /// # Examples
        /// ```
        /// use membank::sync::{BlockingDrop, MemoryBank};
        ///
        /// let bank: MemoryBank<BlockingDrop, Vec<_>> = MemoryBank::default();
        ///
        /// let v = vec![2.1, 4.3, 1.0, 0.98];
        /// let v_clone = v.clone();
        ///
        /// let loan = bank.deposit(v);
        ///
        /// assert_eq!(*loan, v_clone);
        /// ```
        pub fn deposit(&self, value: T) -> Loan<BlockingDrop, T> {
            Loan::new_blocking(value, Arc::downgrade(&self.list))
        }

        /// Only take a loan if it's from previously used memory, if there is no previously allocated
        /// memory, returns `None`.
        /// # Errors
        /// If another user of the bank panicked while holding it, an error will be returned.
        ///
        /// # Example
        /// ```
        /// use membank::sync::{BlockingDrop, MemoryBank};
        ///
        /// let bank: MemoryBank<BlockingDrop, Vec<i32>> = MemoryBank::default();
        /// assert!(bank.take_old_loan().unwrap().is_none());
        ///
        /// let loan1 = bank.take_loan();
        ///
        /// assert!(bank.take_old_loan().unwrap().is_none());
        ///
        /// drop(loan1);
        /// assert!(bank.take_old_loan().unwrap().is_some());
        /// ```
        pub fn take_old_loan(&self) -> LockResult<Option<Loan<BlockingDrop, T>>, T> {
            // TODO: better return type?
            match self.list.lock()?.pop() {
                Some(value) => Ok(Some(Loan::new_blocking(value, Arc::downgrade(&self.list)))),
                None => Ok(None),
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        use std::collections::hash_map::DefaultHasher;

        #[test]
        fn blocking_basics() {
            let bank: MemoryBank<BlockingDrop, Vec<i32>> = MemoryBank::default();

            let mut loan = bank.take_loan().unwrap();
            loan.clone_from(&vec![1, 2, 3, 4, 5]);
            let loan1_capacity = loan.capacity();
            drop(loan);

            assert_eq!(bank.list.lock().unwrap().len(), 1);

            let loan2 = bank.take_loan().unwrap();
            assert!(bank.list.lock().unwrap().is_empty());
            assert_eq!(loan2.capacity(), loan1_capacity);
            assert_eq!(*loan2, vec![1, 2, 3, 4, 5]);
        }

        #[test]
        fn nonblocking_basics() {
            let (tx, rx) = channel::<()>();

            let bank: MemoryBank<NonBlockingDrop, Vec<i32>> =
                MemoryBank::new_nonblocking_with_notifier(tx);

            let v1 = vec![4, 3, 1];

            let mut loan = bank.take_loan().unwrap();
            loan.clone_from(&v1);
            drop(loan);

            assert!(rx.recv().is_ok());

            let loan2 = bank.take_loan().unwrap();
            assert_eq!(*loan2, v1);
        }

        #[test]
        fn blocking_multiloan() {
            let bank: MemoryBank<BlockingDrop, Vec<i32>> = MemoryBank::default();

            let mut loan1 = bank.take_loan().unwrap();
            let mut loan2 = bank.take_loan().unwrap();

            loan1.clone_from(&(0..100).collect());
            loan2.clone_from(&(0..500).collect());

            loan1[5] = 2;
            loan2[3] = 5;

            let loan1_vec = loan1.clone();
            let loan2_vec = loan2.clone();

            let loan2_clone = loan2.clone();
            drop(loan2);
            assert_eq!(loan2_clone, bank.list.lock().unwrap()[0]);

            let loan1_clone = loan1.clone();
            drop(loan1);
            assert_eq!([loan2_clone, loan1_clone], **bank.list.lock().unwrap());

            let loan3 = bank.take_loan().unwrap();
            let loan4 = bank.take_loan().unwrap();

            assert!(bank.list.lock().unwrap().is_empty());

            drop(bank);

            assert_eq!(*loan3, loan1_vec);
            assert_eq!(*loan4, loan2_vec);
        }

        #[test]
        fn nonblocking_multiloan() {
            let (tx, rx) = channel::<()>();

            let bank: MemoryBank<NonBlockingDrop, Vec<i32>> =
                MemoryBank::new_nonblocking_with_notifier(tx);

            let mut loan1 = bank.take_loan().unwrap();
            let mut loan2 = bank.take_loan().unwrap();

            loan1.clone_from(&(0..100).collect());
            loan2.clone_from(&(0..500).collect());

            loan1[5] = 2;
            loan2[3] = 5;

            let loan1_vec = loan1.clone();
            let loan2_vec = loan2.clone();

            let loan2_clone = loan2.clone();
            drop(loan2);
            assert!(rx.recv().is_ok());
            assert_eq!(loan2_clone, bank.list.lock().unwrap()[0]);

            let loan1_clone = loan1.clone();
            drop(loan1);
            assert!(rx.recv().is_ok());
            assert_eq!([loan2_clone, loan1_clone], **bank.list.lock().unwrap());

            let loan3 = bank.take_loan().unwrap();
            let loan4 = bank.take_loan().unwrap();

            assert!(bank.list.lock().unwrap().is_empty());

            drop(bank);

            assert_eq!(*loan3, loan1_vec);
            assert_eq!(*loan4, loan2_vec);
        }

        #[test]
        fn hash_equality() {
            let bank: MemoryBank<BlockingDrop, String> = MemoryBank::default();

            let string = "Swimming with the fishes";

            let mut loan_string = bank.take_loan().unwrap();
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

            let bank: MemoryBank<BlockingDrop, i32> = MemoryBank::default();

            let mut loan1 = bank.take_loan().unwrap();
            *loan1 = n1;

            let mut loan2 = bank.take_loan().unwrap();
            *loan2 = n2;

            assert!(loan1 > loan2);
            assert_ne!(loan1, loan2);
        }

        #[test]
        fn blocking_bank_between_threads() {
            let bank: Arc<MemoryBank<BlockingDrop, Vec<i32>>> = Arc::new(MemoryBank::default());

            let mut v1 = vec![0, 2, 3, 5];

            let mut loan = bank.take_loan_and_clone(&v1).unwrap();
            loan[0] = 1;
            v1[0] = 1;

            assert_eq!(*loan, v1);

            drop(loan);

            let bank_clone = Arc::clone(&bank);
            let v1_clone = v1.clone();

            let thread = std::thread::spawn(move || {
                let mut loan = bank_clone.take_loan().unwrap();

                assert_eq!(*loan, v1_clone);

                loan[1] = 1;
            });

            v1[1] = 1;

            let _ = thread.join();
            let loan = bank.take_loan().unwrap();
            assert_eq!(*loan, v1);
        }

        #[test]
        fn nonblocking_bank_between_threads() {
            let (tx, rx) = channel::<()>();

            let bank: Arc<MemoryBank<NonBlockingDrop, Vec<i32>>> =
                Arc::new(MemoryBank::new_nonblocking_with_notifier(tx));

            let mut v1 = vec![0, 2, 3, 5];

            let mut loan = bank.take_loan_and_clone(&v1).unwrap();
            loan[0] = 1;
            v1[0] = 1;

            assert_eq!(*loan, v1);

            drop(loan);

            let bank_clone = Arc::clone(&bank);
            let v1_clone = v1.clone();

            assert!(rx.recv().is_ok());
            let thread = std::thread::spawn(move || {
                let mut loan = bank_clone.take_loan().unwrap();

                assert_eq!(*loan, v1_clone);

                loan[1] = 1;
            });

            v1[1] = 1;

            let _ = thread.join();
            assert!(rx.recv().is_ok());
            let loan = bank.take_loan().unwrap();
            assert_eq!(*loan, v1);
        }

        #[test]
        fn blocking_loan_between_threads() {
            let bank: MemoryBank<BlockingDrop, Vec<i32>> = MemoryBank::default();

            let mut v = vec![17, 23, 1, 4, 5];

            let mut loan = bank.take_loan_and_clone(&v).unwrap();
            loan[0] = 3;
            v[0] = 3;

            let v_clone = v.clone();
            let thread = std::thread::spawn(move || {
                assert_eq!(*loan, v_clone);

                loan[2] = 7;
            });

            v[2] = 7;
            let _ = thread.join();
            let loan = bank.take_loan().unwrap();
            assert_eq!(*loan, v);
        }

        #[test]
        fn nonblocking_loan_between_threads() {
            let (tx, rx) = channel::<()>();

            let bank: MemoryBank<NonBlockingDrop, Vec<i32>> =
                MemoryBank::new_nonblocking_with_notifier(tx);

            let mut v = vec![17, 23, 1, 4, 5];

            let mut loan = bank.take_loan_and_clone(&v).unwrap();
            loan[0] = 3;
            v[0] = 3;

            let v_clone = v.clone();
            let thread = std::thread::spawn(move || {
                assert_eq!(*loan, v_clone);

                loan[2] = 7;
            });

            v[2] = 7;
            let _ = thread.join();
            assert!(rx.recv().is_ok());
            let loan = bank.take_loan().unwrap();
            assert_eq!(*loan, v);
        }
    }
}
