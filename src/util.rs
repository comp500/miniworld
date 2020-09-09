pub struct PackedIntegerArrayIter<'a, I: Iterator<Item = &'a i64>> {
	inner: I,
	curr_value: u64,
	curr_offset: u8,
	num_bits: u8,
	bitmask: u64,
}

impl<'a, I: Iterator<Item = &'a i64>> PackedIntegerArrayIter<'a, I> {
	pub fn new(iter: I, num_bits: u8) -> PackedIntegerArrayIter<'a, I> {
		assert!(num_bits > 0, "Number of bits per integer must be greater than 0");
		assert!(num_bits <= 32, "Number of bits per integer must not exceed 32");
		PackedIntegerArrayIter {
			inner: iter,
			curr_value: 0,
			curr_offset: 0,
			num_bits,
			bitmask: (1 << num_bits) - 1
		}
	}
}

impl<'a, I: Iterator<Item = &'a i64>> Iterator for PackedIntegerArrayIter<'a, I> {
	type Item = u32;

	fn next(&mut self) -> Option<Self::Item> {
		// If we are at the start, read the next long
		if self.curr_offset == 0 {
			self.curr_value = *self.inner.next()? as u64;
		}
		// Shift to get the value, mask it to get only the bits we want
		let value = ((self.curr_value >> self.curr_offset) & self.bitmask) as u32;
		// Move to the next value
		self.curr_offset += self.num_bits;
		if self.curr_offset == (64 - (64 % self.num_bits)) {
			self.curr_offset = 0;
		}
		Some(value)
	}
}
