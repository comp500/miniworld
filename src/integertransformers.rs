use core::num;
use std::convert::TryInto;

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