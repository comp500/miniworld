use byteorder::{BigEndian, ReadBytesExt};
use io::{BufWriter, Seek, SeekFrom};
use nbt::{Blob, Value};
use std::{
	fs::File,
	io::{self, BufReader},
	path::Path,
};

#[derive(Debug, Copy, Clone)]
struct ChunkPosition {
	offset: u32,
	sector_count: u8,
}

#[derive(Debug, Clone)]
struct PaletteValue {
	color: [u8; 3],
	nbt: Value,
}

fn main() -> anyhow::Result<()> {
	let orig_path = Path::new("r.1.2.mca");
	let file = File::open(orig_path)?;
	let mut buf_reader = BufReader::new(file);

	let mut target_array = vec![0u8; 131072];

	let mut palette_list = vec![];

	let mut chunk_positions = vec![];

	for _ in 0..1024 {
		let value = buf_reader.read_u32::<BigEndian>()?;
		let offset = value >> 8;
		let sector_count = (value & 0b1111_1111) as u8;

		if sector_count == 0 || offset == 0 {
			continue;
		}

		chunk_positions.push(ChunkPosition { offset, sector_count });
	}

	println!("Found {} chunks", chunk_positions.len());

	for curr_x in 0..511 {
		let new_path_name = format!("test{}.png", curr_x);
		let new_path = Path::new(new_path_name.as_str());
		let new_file = File::create(new_path)?;
		let mut buf_writer = BufWriter::new(new_file);
		let mut encoder = png::Encoder::new(&mut buf_writer, 512, 256);
		encoder.set_color(png::ColorType::Indexed);

		for pos in &chunk_positions {
			buf_reader.seek(SeekFrom::Start((pos.offset * 4096) as u64))?;
			let _length = buf_reader.read_u32::<BigEndian>()?;
			let _compression_type = buf_reader.read_u8()?;
			// TODO: handle non-zlib

			let chunk_data = Blob::from_zlib_reader(&mut buf_reader)?;
			transform_chunk(&chunk_data, &mut target_array, &mut palette_list, curr_x)?;
		}

		println!("Generated palette of {} blockstates", palette_list.len());
		let mut palette_bytes = vec![];
		for palette_element in &palette_list {
			palette_bytes.push(palette_element.color[0]);
			palette_bytes.push(palette_element.color[1]);
			palette_bytes.push(palette_element.color[2]);
		}
		encoder.set_palette(palette_bytes);
		let mut writer = encoder.write_header()?;
		writer.write_image_data(&target_array)?;
		println!("Successfully written {} blocks to PNG", 262144 * 256);

		target_array = vec![0u8; 131072];
	}

	Ok(())
}

fn transform_chunk(data: &Blob, target_array: &mut Vec<u8>, img_palette: &mut Vec<PaletteValue>, curr_x: usize) -> anyhow::Result<()> {
	if let Some(Value::Compound(level)) = data.get("Level") {
		if let (Some(Value::List(sections)), Some(Value::Int(x_pos)), Some(Value::Int(z_pos))) =
			(level.get("Sections"), level.get("xPos"), level.get("zPos"))
		{
			for section in sections {
				transform_chunk_section(section, target_array, img_palette, *x_pos, *z_pos, curr_x);
			}
		}
	}
	Ok(())
}

fn transform_chunk_section(
	data: &Value,
	target_array: &mut Vec<u8>,
	img_palette: &mut Vec<PaletteValue>,
	chunk_x: i32,
	chunk_z: i32,
	curr_x: usize
) {
	if let Value::Compound(map) = data {
		if let Some(Value::List(palette)) = map.get("Palette") {
			let palette_length = palette.len();

			let num_bits = match (palette_length as f64).log2().ceil() as usize {
				0..=4 => 4,
				x => x,
			};

			let mut palette_map = vec![0u8; img_palette.len()];

			'outer: for (i, palette_element) in palette.iter().enumerate() {
				for (j, img_palette_element) in img_palette.iter().enumerate() {
					if *palette_element == img_palette_element.nbt {
						palette_map.insert(i, j as u8);
						continue 'outer;
					}
				}
				let color = palette::Srgb::from(palette::Hsv::new((img_palette.len() as f32 / 162.0) * 360.0, 1.0, 1.0));
				palette_map.insert(i, img_palette.len() as u8);
				img_palette.push(PaletteValue {
					color: [(color.red * 255.0) as u8, (color.green * 255.0) as u8, (color.blue * 255.0) as u8],
					nbt: palette_element.clone(),
				});
			}

			if let (Some(Value::LongArray(states)), Some(Value::Byte(section_y))) = (map.get("BlockStates"), map.get("Y")) {
				let mut iter = PackedIntegerArrayIter::new(states.iter(), num_bits as u8)
					.map(|value| value as usize)
					.inspect(|value| assert!(*value < palette_length, "Invalid palette value"))
					.map(|value| palette_map[value]);

				// & 31 makes the coordinates region-relative
				let chunk_x_mul = ((chunk_x & 31) * 16) as usize;
				let chunk_z_mul = ((chunk_z & 31) * 16) as usize;
				let section_off = (*section_y as usize) * 16;

				for y in section_off..(section_off + 16) {
					for z in chunk_z_mul..(chunk_z_mul + 16) {
						for x in chunk_x_mul..(chunk_x_mul + 16) {
							if x == curr_x {
								// Flip y so sky is at the top :)
								target_array[((255 - y) * 512) + z] = iter.next().unwrap();
							} else {
								iter.next().unwrap();
							}
						}
					}
				}
			}
		}
	}
}

struct PackedIntegerArrayIter<'a, I: Iterator<Item = &'a i64>> {
	inner: I,
	curr_value: u64,
	curr_offset: u8,
	num_bits: u8,
	bitmask: u64,
}

impl<'a, I: Iterator<Item = &'a i64>> PackedIntegerArrayIter<'a, I> {
	fn new(iter: I, num_bits: u8) -> PackedIntegerArrayIter<'a, I> {
		assert!(num_bits > 0, "Number of bits per integer must be greater than 0");
		assert!(num_bits <= 32, "Number of bits per integer must not exceed 32");
		PackedIntegerArrayIter {
			inner: iter,
			curr_value: 0,
			curr_offset: 0,
			num_bits,
			bitmask: (1 << num_bits) - 1,
		}
	}
}

impl<'a, I: Iterator<Item = &'a i64>> Iterator for PackedIntegerArrayIter<'a, I> {
	type Item = u32;

	fn next(&mut self) -> Option<Self::Item> {
		// If we are at the end (no more bits to shift) go back to the start (of the next long)
		if self.curr_offset == 0 {
			self.curr_offset = 64;
		}
		// If we are at the start (no bits have been shifted) get a new long from the inner iter
		if self.curr_offset == 64 {
			self.curr_value = *self.inner.next()? as u64;
			// Skip padding
			self.curr_offset = 64 - (64 % self.num_bits);
		}
		// Move to the next value
		self.curr_offset -= self.num_bits;
		// Shift to get the next value, mask it to get only the bits we want
		Some(((self.curr_value >> self.curr_offset) & self.bitmask) as u32)
	}
}
