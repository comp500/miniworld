use core::num;
use std::{convert::TryInto, marker::PhantomData};

use fixed_vec_deque::FixedVecDeque;
use hilbert_index::{FromHilbertIndex, ToHilbertIndex};

pub trait IntegerTransformer {
	fn transform(data: &mut[u32; 4096], palette_size: &mut u32);
	fn reverse(data: &mut[u32; 4096], palette_size: &mut u32);
}

pub struct DeltaLeft;

impl IntegerTransformer for DeltaLeft {
    fn transform(data: &mut[u32; 4096], palette_size: &mut u32) {
		let num_bits = match (*palette_size as f64).log2().ceil() as usize {
			0..=4 => 4,
			x => x,
		};
		let mask = (1 << num_bits) - 1;

		let mut prev = 0u32;
        for v in data {
			let vcopy = *v;
			*v = (vcopy.wrapping_sub(prev)) & mask;
			prev = vcopy;
		}

		// Increase palette size to full num_bits range
		*palette_size = 1 << num_bits;
    }

    fn reverse(data: &mut[u32; 4096], palette_size: &mut u32) {
		let num_bits = match (*palette_size as f64).log2().ceil() as usize {
			0..=4 => 4,
			x => x,
		};
		let mask = (1 << num_bits) - 1;

        let mut prev = 0u32;
        for v in data {
			let delta = *v;
			*v = (prev.wrapping_add(prev)) & mask;
			prev = *v;
		}

		// Increase palette size to full num_bits range
		*palette_size = 1 << num_bits;
    }
}

pub struct MoveToFront;

impl IntegerTransformer for MoveToFront {
    fn transform(data: &mut[u32; 4096], palette_size: &mut u32) {
		let mut statemap: Vec<u32> = (0..*palette_size).collect();
        
		for v in data {
			let value = *v;
			if value >= *palette_size {
				panic!("Bad value! {}", value);
			}
			let curr_pos = statemap.iter().position(|state| *state == value).unwrap() as u32;
			*v = curr_pos;
			
			statemap.remove(curr_pos.try_into().unwrap());
			statemap.insert(0, value);
		}
    }

    fn reverse(data: &mut[u32; 4096], palette_size: &mut u32) {
		let mut statemap: Vec<u32> = (0..*palette_size).collect();

        for v in data {
			let curr_pos = *v;
			*v = statemap[curr_pos as usize];
			
			let value = statemap.remove(curr_pos.try_into().unwrap());
			statemap.insert(0, value);
		}
    }
}

pub struct None;

impl IntegerTransformer for None {
    fn transform(_data: &mut[u32; 4096], _palette_size: &mut u32) {
        // Do nothing!
    }

    fn reverse(_data: &mut[u32; 4096], _palette_size: &mut u32) {
        // Do nothing!
    }
}

pub struct MoveToFrontLookbehind;

impl IntegerTransformer for MoveToFrontLookbehind {
    fn transform(data: &mut[u32; 4096], palette_size: &mut u32) {
		let mut statemap: Vec<u32> = (0..*palette_size).collect();
		let mut lookbehind = FixedVecDeque::<[u32; 256]>::new();
		// Add 2 new symbols referring to the values 16 and 256 behind respectively
		let sym_behind_16 = *palette_size;
		let sym_behind_256 = *palette_size + 1;
        
		for v in data {
			let value = *v;
			if value >= *palette_size {
				panic!("Bad value! {}", value);
			}

			let curr_pos = statemap.iter().position(|state| *state == value).unwrap() as u32;
			if curr_pos == 0 {
				*v = curr_pos;
			} else if value == lookbehind_or_zero(&lookbehind, 15) {
				*v = sym_behind_16;
			} else if value == lookbehind_or_zero(&lookbehind, 255) {
				*v = sym_behind_256;
			} else {
				*v = curr_pos;
			}
			*lookbehind.push_front() = value;
			
			statemap.remove(curr_pos.try_into().unwrap());
			statemap.insert(0, value);
		}

		*palette_size += 2;
    }

    fn reverse(data: &mut[u32; 4096], palette_size: &mut u32) {
		let mut statemap: Vec<u32> = (0..*palette_size).collect();
		let mut lookbehind = FixedVecDeque::<[u32; 256]>::new();
		// Add 2 new symbols referring to the values 16 and 256 behind respectively
		let sym_behind_16 = *palette_size;
		let sym_behind_256 = *palette_size + 1;
		*palette_size += 2;

        for v in data {
			let curr_pos = if *v == sym_behind_16 {
				let value = lookbehind_or_zero(&lookbehind, 15);
				statemap.iter().position(|state| *state == value).unwrap() as u32
			} else if *v == sym_behind_256 {
				let value = lookbehind_or_zero(&lookbehind, 255);
				statemap.iter().position(|state| *state == value).unwrap() as u32
			} else {
				*v
			};

			*v = statemap[curr_pos as usize];
			*lookbehind.push_front() = *v;
			
			let value = statemap.remove(curr_pos.try_into().unwrap());
			statemap.insert(0, value);
		}
    }
}

fn lookbehind_or_zero(buf: &FixedVecDeque<[u32; 256]>, index: usize) -> u32 {
	match buf.get(index) {
		Some(v) => *v,
		Option::None => 0
	}
}

pub struct ZOrderCurve;

impl IntegerTransformer for ZOrderCurve {
    fn transform(data: &mut[u32; 4096], _palette_size: &mut u32) {
		let copy = data.clone();
		for (i, v) in copy.iter().enumerate() {
			data[interleave_idx(i as u32) as usize] = *v;
		}
    }

    fn reverse(data: &mut[u32; 4096], _palette_size: &mut u32) {
		let copy = data.clone();
		for (i, v) in copy.iter().enumerate() {
			data[uninterleave_idx(i as u32) as usize] = *v;
		}
    }
}

fn interleave_idx(i: u32) -> u32 {
	// YZX ordering
	interleave_xyz(i & 15, (i >> 8) & 15, (i >> 4) & 15)
}

fn interleave_xyz(x: u32, y: u32, z: u32) -> u32 {
	interleave_4_bits(x) | (interleave_4_bits(y) << 2) | (interleave_4_bits(z) << 1)
}

fn interleave_4_bits(mut x: u32) -> u32 {
	let mut v = 0;
	// 3 components, 4 bits
	for shift in (0..4*3).step_by(3) {
		v |= (x & 1) << shift;
		x >>= 1;
	}
	v
}

fn uninterleave_idx(i: u32) -> u32 {
	get_idx(uninterleave_4_bits(i), uninterleave_4_bits(i >> 2), uninterleave_4_bits(i >> 1))
}

fn uninterleave_4_bits(mut x: u32) -> u32 {
	let mut v = 0;
	// 3 components, 4 bits
	for shift in 0..4 {
		v |= (x & 1) << shift;
		x >>= 3;
	}
	v
}

fn get_idx(x: u32, y: u32, z: u32) -> u32 {
	y << 8 | z << 4 | x
}

impl<A: IntegerTransformer, B: IntegerTransformer> IntegerTransformer for (A, B) {
	fn transform(data: &mut[u32; 4096], palette_size: &mut u32) {
		A::transform(data, palette_size);
		B::transform(data, palette_size);
    }

    fn reverse(data: &mut[u32; 4096], palette_size: &mut u32) {
		B::reverse(data, palette_size);
		A::reverse(data, palette_size);
    }
}

pub struct HilbertCurve;

const HILBERT_LEVEL: usize = 4;

impl IntegerTransformer for HilbertCurve {
    fn transform(data: &mut[u32; 4096], _palette_size: &mut u32) {
		let copy = data.clone();
		for (i, v) in copy.iter().enumerate() {
			// X and Y swapped for better locality
			data[[(i >> 8) & 15, i & 15, (i >> 4) & 15].to_hilbert_index(HILBERT_LEVEL)] = *v;
		}
    }

    fn reverse(data: &mut[u32; 4096], _palette_size: &mut u32) {
		let copy = data.clone();
		for (i, v) in copy.iter().enumerate() {
			let [x, y, z] = i.from_hilbert_index(HILBERT_LEVEL);
			// X and Y swapped for better locality
			data[get_idx(y as u32, x as u32, z as u32) as usize] = *v;
		}
    }
}

pub struct HilbertCurveAdaptive;

impl IntegerTransformer for HilbertCurveAdaptive {
    fn transform(data: &mut[u32; 4096], palette_size: &mut u32) {
		let copy = data.clone();
		let mut run_count_no_curve = 0;
		let mut run_count = 0;
		let mut last_v = *palette_size + 1;

		// For each symbol where the value is the same as the previous, increment the run count
		for v in copy {
			if v == last_v {
				run_count_no_curve += 1;
			}
			last_v = v;
		}

		for (i, v) in copy.iter().enumerate() {
			// X and Y swapped for better locality
			data[[(i >> 8) & 15, i & 15, (i >> 4) & 15].to_hilbert_index(HILBERT_LEVEL)] = *v;
		}

		last_v = *palette_size + 1;
		for v in data.iter() {
			if *v == last_v {
				run_count += 1;
			}
			last_v = *v;
		}

		// Compare the run counts before and after the hilbert transform - use the pre-transform array if it has a greater run count
		if run_count_no_curve > run_count {
			for (i, v) in copy.iter().enumerate() {
				data[i] = *v;
			}
		}
    }

    fn reverse(data: &mut[u32; 4096], _palette_size: &mut u32) {
		// TODO: need an extra value to say whether the curve was used!
		let copy = data.clone();
		for (i, v) in copy.iter().enumerate() {
			let [x, y, z] = i.from_hilbert_index(HILBERT_LEVEL);
			// X and Y swapped for better locality
			data[get_idx(y as u32, x as u32, z as u32) as usize] = *v;
		}
    }
}