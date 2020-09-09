use flate2::bufread::ZlibDecoder;
use byteorder::{BigEndian, ReadBytesExt};
use io::{BufWriter, Seek, SeekFrom, Read, Cursor};
use nbt::{Blob, Value};
use std::{
	fs::File,
	io::{self, BufReader},
	path::Path,
};
use humansize::FileSize;

mod util;

use util::PackedIntegerArrayIter;

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
	let mut total_original_size = 0;
	let mut total_unpadded_size = 0;
	let mut total_decompressed_size = 0;
	let mut total_recompressed_size = 0;

	//let orig_path = Path::new("r.1.2.mca");
	//benchmark_file(orig_path, &mut total_original_size, &mut total_unpadded_size, &mut total_decompressed_size, &mut total_recompressed_size)?;

	for file in std::fs::read_dir(Path::new("bench"))? {
		let file = file?;
		println!("Reading file {:?}", &file.path());
		benchmark_file(&file.path(), &mut total_original_size, &mut total_unpadded_size, &mut total_decompressed_size, &mut total_recompressed_size)?;
	}

	println!("Total original size: {}", total_original_size.file_size(humansize::file_size_opts::DECIMAL).unwrap());
	println!("Total unpadded size: {}", total_unpadded_size.file_size(humansize::file_size_opts::DECIMAL).unwrap());
	println!("Total decompressed size: {}", total_decompressed_size.file_size(humansize::file_size_opts::DECIMAL).unwrap());
	println!("Total recompressed size: {} (zstd level 18)", total_recompressed_size.file_size(humansize::file_size_opts::DECIMAL).unwrap());

	Ok(())
}

fn benchmark_file(orig_path: &Path, total_original_size: &mut u64, total_unpadded_size: &mut u64, total_decompressed_size: &mut u64, total_recompressed_size: &mut u64) -> anyhow::Result<()> {
	let file = File::open(orig_path)?;
	let mut buf_reader = BufReader::new(file);

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

	let orig_size = buf_reader.seek(SeekFrom::End(0))?;
	*total_original_size += orig_size;
	println!("Original size: {}", orig_size.file_size(humansize::file_size_opts::DECIMAL).unwrap());
	let mut unpadded_size = 0;
	let mut decompressed_size = 0;

	let mut decompress_dest = vec![];

	for pos in &chunk_positions {
		buf_reader.seek(SeekFrom::Start((pos.offset * 4096) as u64))?;
		let _length = buf_reader.read_u32::<BigEndian>()?;
		let _compression_type = buf_reader.read_u8()?;
		// TODO: handle non-zlib

		let mut decoder = ZlibDecoder::new(&mut buf_reader);
		//let chunk_data = Blob::from_zlib_reader(&mut buf_reader)?;

		decoder.read_to_end(&mut decompress_dest)?;

		unpadded_size += decoder.total_in();
		decompressed_size += decoder.total_out();
		
		//transform_chunk(&chunk_data, &mut target_array, &mut palette_list, curr_x)?;
	}

	println!("Unpadded size: {}", unpadded_size.file_size(humansize::file_size_opts::DECIMAL).unwrap());
	println!("Decompressed size: {}", decompressed_size.file_size(humansize::file_size_opts::DECIMAL).unwrap());
	*total_unpadded_size += unpadded_size;
	*total_decompressed_size += decompressed_size;

	// {
	// 	let mut recompress_src = Cursor::new(&decompress_dest);
	// 	let mut recompress_dest = vec![];

	// 	brotli::BrotliCompress(&mut recompress_src, &mut recompress_dest, &brotli::enc::BrotliEncoderParams::default())?;

	// 	println!("Recompressed size: {} (brotli)", recompress_dest.len().file_size(humansize::file_size_opts::DECIMAL).unwrap());
	// }

	{
		let mut recompress_src = Cursor::new(&decompress_dest);
		let mut recompress_dest = vec![];

		zstd::stream::copy_encode(&mut recompress_src, &mut recompress_dest, 18)?;

		println!("Recompressed size: {} (zstd level 18)", recompress_dest.len().file_size(humansize::file_size_opts::DECIMAL).unwrap());
		*total_recompressed_size += recompress_dest.len() as u64;
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

				if let Some(remaining_value) = iter.next() {
					if remaining_value > 0 {
						panic!("found remaining value from packed integer array iterator: {}
palette size: {}
chunk x/z: {} {}
chunk x/z mul: {} {}
section offset: {}", remaining_value, palette_length, chunk_x, chunk_z, chunk_x_mul, chunk_z_mul, section_off);
					}
				}
			}
		}
	}
}