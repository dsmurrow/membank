# membank


The membank crate allows users to re-use memory for later instead of de-allocating it. This is great for large, heap-allocated data.


```rust
use membank::unsync::MemoryBank;

fn main() {
	let mut bank: MemoryBank<Vec<i32>> = MemoryBank::new();

	let mut big_dumb_vec = Vec::from_iter(0..10000);

	// First loan is always a new allocation
	let mut loan = bank.take_loan();
	loan.clone_from(&big_dumb_vec);

	assert_eq!(*loan, big_dumb_vec);

	// The memory in the loan is mutable
	loan[9990] = 14;

	big_dumb_vec[9990] = 14;
	assert_eq!(*loan, big_dumb_vec);

	// The loaned memory is returned to the bank
	drop(loan);

	// The bank reuses memory whenever it can
	let same_memory = bank.take_loan();
	assert_eq!(*same_memory, big_dumb_vec);
}
```

