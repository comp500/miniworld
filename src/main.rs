use arcode::{encode::encoder::ArithmeticEncoder, util::source_model::SourceModel};
use bitbit::{BitReader, BitWriter, MSB};
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use flate2::bufread::ZlibDecoder;
use io::{BufWriter, Cursor, Read, Seek, SeekFrom, Write};
use nbt::{Blob, Value};
use std::{
	fs::File,
	io::{self, BufReader},
	path::Path,
};
use std::collections::HashMap;

#[derive(Debug, Copy, Clone)]
struct ChunkPosition {
	offset: u32,
	sector_count: u8,
}

#[derive(Debug, Clone)]
struct PaletteValue {
	color: [u8; 3],
	nbt: Value
}

fn main() -> anyhow::Result<()> {
	let orig_path = Path::new("r.1.2.mca");
	let file = File::open(orig_path)?;
	let mut buf_reader = BufReader::new(file);

	let new_path = Path::new("test.png");
	let new_file = File::create(new_path)?;
	let ref mut buf_writer = BufWriter::new(new_file);
	let mut encoder = png::Encoder::new(buf_writer, 262144, 256);
	encoder.set_color(png::ColorType::Indexed);
	// TODO: set palette
	let mut target_array = vec![0u8; 67108864];

	let mut palette_list = vec![];

	let mut chunk_positions = vec![];

	for _ in 0..1024 {
		let value = buf_reader.read_u32::<BigEndian>()?;
		let offset = value >> 8;
		let sector_count = (value & 0b1111_1111) as u8;

		if sector_count == 0 || offset == 0 {
			continue;
		}

		chunk_positions.push(ChunkPosition {
			offset: offset,
			sector_count: sector_count,
		});
	}

	println!("Found {} chunks", chunk_positions.len());

	for pos in chunk_positions {
		buf_reader.seek(SeekFrom::Start((pos.offset * 4096) as u64))?;
		let length = buf_reader.read_u32::<BigEndian>()?;
		let compression_type = buf_reader.read_u8()?;
		// TODO: handle non-zlib

		let chunk_data = Blob::from_zlib_reader(&mut buf_reader)?;
		transform_chunk(&chunk_data, &mut target_array, &mut palette_list)?;
	}

	println!("Generated palette of {} blockstates", palette_list.len());
	let mut palette_bytes = vec![];
	for palette_element in palette_list {
		palette_bytes.push(palette_element.color[0]);
		palette_bytes.push(palette_element.color[1]);
		palette_bytes.push(palette_element.color[2]);
	}
	encoder.set_palette(palette_bytes);
	let mut writer = encoder.write_header()?;
	writer.write_image_data(&target_array)?;
	println!("Successfully written {} blocks to PNG", 262144 * 256);

	Ok(())
}

fn transform_chunk(data: &Blob, target_array: &mut Vec<u8>, img_palette: &mut Vec<PaletteValue>) -> anyhow::Result<()> {
	if let Some(Value::Compound(level)) = data.get("Level") {
		if let (
			Some(Value::List(sections)),
			Some(Value::Int(x_pos)),
			Some(Value::Int(z_pos))
		) = (level.get("Sections"), level.get("xPos"), level.get("zPos")) {
			for section in sections {
				transform_chunk_section(section, target_array, img_palette, *x_pos, *z_pos);
			}
		}
	}
	Ok(())
}

fn transform_chunk_section(data: &Value, target_array: &mut Vec<u8>, img_palette: &mut Vec<PaletteValue>, chunk_x: i32, chunk_z: i32) {
	if let Value::Compound(map) = data {
		if let Some(Value::List(palette)) = map.get("Palette") {
			let palette_length = palette.len();

			let num_bits = match (palette_length as f64).log2().ceil() as usize {
				0..=4 => 4,
				x => x
			};

			let mut palette_map = HashMap::new();

			'outer: for (i, palette_element) in palette.iter().enumerate() {
				for (j, img_palette_element) in img_palette.iter().enumerate() {
					if *palette_element == img_palette_element.nbt {
						palette_map.insert(i, j as u8);
						continue 'outer;
					}
				}
				let color = random_color::RandomColor::new().to_rgb_array();
				palette_map.insert(i, img_palette.len() as u8);
				img_palette.push(PaletteValue {
					color: [color[0] as u8, color[1] as u8, color[2] as u8],
					nbt: palette_element.clone()
				});
			}

			if let (
				Some(Value::LongArray(states)),
				Some(Value::Byte(section_y))
			) = (map.get("BlockStates"), map.get("Y")) {
				let mut iter = PackedIntegerArrayIter::new(states.iter().cloned(), num_bits as u8);

				// & 31 makes the coordinates region-relative
				let chunk_x_mul = ((chunk_x & 31) * 16) as usize;
				let chunk_z_mul = ((chunk_z & 31) * 16) as usize;
				let section_off = (*section_y as usize) * 16;

				for y in section_off..(section_off + 16) {
					for z in chunk_z_mul..(chunk_z_mul + 16) {
						for x in chunk_x_mul..(chunk_x_mul + 16) {
							let value = iter.next().unwrap() as usize;
							assert!(value < palette_length);
							let img_value = palette_map.get(&value).unwrap();
							// Flip y so sky is at the top :)
							target_array[((255 - y) * 262144) + (x * 512) + z] = *img_value;
						}
					}
				}
			}
		}
	}
}

struct PackedIntegerArrayIter<I: Iterator<Item = i64>> {
	inner: I,
	curr_value: u64,
	curr_offset: u8,
	num_bits: u8,
	bitmask: u64
}

impl<I: Iterator<Item = i64>> PackedIntegerArrayIter<I> {
	fn new(iter: I, num_bits: u8) -> PackedIntegerArrayIter<I> {
		assert!(num_bits > 0, "Number of bits per integer must be greater than 0");
		assert!(num_bits <= 32, "Number of bits per integer must not exceed 32");
		PackedIntegerArrayIter {
			inner: iter,
			curr_value: 0,
			curr_offset: 0,
			num_bits: num_bits,
			bitmask: (1 << num_bits) - 1
		}
	}
}

impl<I: Iterator<Item = i64>> Iterator for PackedIntegerArrayIter<I> {
    type Item = u32;

    fn next(&mut self) -> Option<Self::Item> {
		if self.curr_offset >= 64 {
			self.curr_offset = 0;
		}
		if self.curr_offset == 0 {
			self.curr_value = self.inner.next()? as u64;
			// Skip padding
			self.curr_offset = 64 % self.num_bits;
		}
		self.curr_offset += self.num_bits;
		Some(((self.curr_value >> (64 - self.curr_offset)) & self.bitmask) as u32)
    }
}