#![feature(drain_filter)]

use flate2::bufread::ZlibDecoder;
use byteorder::{BigEndian, ReadBytesExt, ByteOrder};
use io::{BufWriter, Seek, SeekFrom, Read, Cursor, Write};
use nbt::{Blob, Value};
use std::{
	fs::File,
	io::{self, BufReader},
	path::Path,
time::Instant, collections::BTreeMap};
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

	let mut palette_frequencies = [0u64; 256];
	let mut root_sizes = SizeNode::Parent(0, BTreeMap::new());
	let mut blockstate_value_counts = [0u64; 1024];

	//let orig_path = Path::new("r.1.2.mca");
	//benchmark_file(orig_path, &mut total_original_size, &mut total_unpadded_size, &mut total_decompressed_size, &mut total_recompressed_size)?;

	for file in std::fs::read_dir(Path::new("bench"))? {
		let file = file?;
		println!("Reading file {:?}", &file.path());
		benchmark_file(&file.path(), &mut total_original_size, &mut total_unpadded_size, &mut total_decompressed_size, &mut total_recompressed_size, &mut palette_frequencies, &mut root_sizes, &mut blockstate_value_counts)?;
	}

	println!("Total original size: {}", total_original_size.file_size(humansize::file_size_opts::DECIMAL).unwrap());
	println!("Total unpadded size: {}", total_unpadded_size.file_size(humansize::file_size_opts::DECIMAL).unwrap());
	println!("Total decompressed size: {}", total_decompressed_size.file_size(humansize::file_size_opts::DECIMAL).unwrap());
	println!("Total recompressed size: {} (xz/LZMA)", total_recompressed_size.file_size(humansize::file_size_opts::DECIMAL).unwrap());
	println!("Palette frequencies: {:?}", palette_frequencies);
	println!("Blockstate value frequencies: {:?}", blockstate_value_counts);

	fn print_tree(node: &SizeNode, depth: usize, name: &str) {
		println!("{}{} {}", "    ".repeat(depth), name, node.get_size().file_size(humansize::file_size_opts::DECIMAL).unwrap());
		if let SizeNode::Parent(_size, children) = node {
			for child in children {
				print_tree(child.1, depth + 1, child.0);
			}
		}
	}
	print_tree(&root_sizes, 0, "Level");

	Ok(())
}

fn benchmark_file(orig_path: &Path, total_original_size: &mut u64, total_unpadded_size: &mut u64, total_decompressed_size: &mut u64, total_recompressed_size: &mut u64, palette_frequencies: &mut [u64;256], root_sizes: &mut SizeNode, blockstate_value_counts: &mut[u64; 1024]) -> anyhow::Result<()> {
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
	let mut reencoded_size = 0;
	let mut reencoded_size_of_vec = 0;

	let mut decompress_dest = vec![];
	let mut decompress_chunk_dest = vec![];

	let mut decompress_blockstates_dest = vec![];

	for pos in &chunk_positions {
		buf_reader.seek(SeekFrom::Start((pos.offset * 4096) as u64))?;
		let _length = buf_reader.read_u32::<BigEndian>()?;
		let _compression_type = buf_reader.read_u8()?;
		// TODO: handle non-zlib

		let mut decoder = ZlibDecoder::new(&mut buf_reader);
		//let chunk_data = Blob::from_zlib_reader(&mut buf_reader)?;

		decoder.read_to_end(&mut decompress_chunk_dest)?;

		unpadded_size += decoder.total_in();
		decompressed_size += decoder.total_out();
		
		//transform_chunk(&chunk_data, &mut target_array, &mut palette_list, curr_x)?;

		let mut reencode_src = Cursor::new(&decompress_chunk_dest);
		let mut chunk_data = Blob::from_reader(&mut reencode_src)?;

		if let Some(value) = chunk_data.get("Level") {
			let mut value = value.clone();
			if let Value::Compound(ref mut level_map) = value {
				if let Some(Value::List(sections)) = level_map.get_mut("Sections") {
					sections.drain_filter(|section| {
						if let Value::Compound(section) = section {
							// TODO: lossy! minecraft might not recalculate this data
							// section.remove("BlockLight");
							// section.remove("SkyLight");
							if let Some(Value::List(palette)) = section.get("Palette") {
								let palette_length = palette.len();
								if palette_length == 1 {
									if let Value::Compound(map) = &palette[0] {
										if map["Name"] != Value::String("minecraft:air".to_owned()) {
											panic!("ohno {:?}", map["Name"]);
										}
									}
									return true;
								}
								palette_frequencies[palette_length] = palette_frequencies[palette_length] + 1;
								if let Some(Value::LongArray(mut data)) = section.remove("BlockStates") {
									blockstates_stats(&data, palette_length, blockstate_value_counts);
									decompress_blockstates_dest.append(&mut data);
								}
							} else {
								// TODO: check minecraft's isEmpty
								// Remove sections with no palette (sometimes there is just a Y and no actual data)
								return true;
							}
						}
						false
					});
				}
			}

			chunk_data.insert("Level", value)?;
		}

		if let Some(value) = chunk_data.get("Level") {
			root_sizes.add_size(value.len_bytes());
			build_size_tree_children(value, root_sizes);
		}

		let mut reencode_dest = vec![];
		chunk_data.to_writer(&mut reencode_dest)?;
		reencoded_size += chunk_data.len_bytes();
		reencoded_size_of_vec += reencode_dest.len();

		decompress_dest.append(&mut reencode_dest);
		decompress_chunk_dest.clear();
	}

	let decompress_blockstates_len = decompress_blockstates_dest.len() * 8;
	let mut decompress_blockstates_dest_tmp = vec![0u8;decompress_blockstates_dest.len() * 8];
	BigEndian::write_i64_into(&decompress_blockstates_dest, &mut decompress_blockstates_dest_tmp);
	decompress_dest.append(&mut decompress_blockstates_dest_tmp);

	println!("Unpadded size: {}", unpadded_size.file_size(humansize::file_size_opts::DECIMAL).unwrap());
	println!("Decompressed size: {}", decompressed_size.file_size(humansize::file_size_opts::DECIMAL).unwrap());
	println!("Reencoded size: {}", reencoded_size.file_size(humansize::file_size_opts::DECIMAL).unwrap());
	println!("Reencoded size of vec: {}", reencoded_size_of_vec.file_size(humansize::file_size_opts::DECIMAL).unwrap());
	println!("Blockstates size: {}", decompress_blockstates_len.file_size(humansize::file_size_opts::DECIMAL).unwrap());
	*total_unpadded_size += unpadded_size;
	*total_decompressed_size += decompressed_size;

	// {
	// 	let mut recompress_src = Cursor::new(&decompress_dest);
	// 	let mut recompress_dest = vec![];
	// 	let now = Instant::now();

	// 	brotli::BrotliCompress(&mut recompress_src, &mut recompress_dest, &brotli::enc::BrotliEncoderParams::default())?;

	// 	println!("Recompressed size: {} (brotli)", recompress_dest.len().file_size(humansize::file_size_opts::DECIMAL).unwrap());
	// 	println!("Took {} ms", now.elapsed().as_millis());
	// }

	// {
	// 	let mut recompress_src = Cursor::new(&decompress_dest);
	// 	let mut recompress_dest = vec![];
	// 	let now = Instant::now();

	// 	zstd::stream::copy_encode(&mut recompress_src, &mut recompress_dest, 22)?;

	// 	println!("Recompressed size: {} (zstd level 22)", recompress_dest.len().file_size(humansize::file_size_opts::DECIMAL).unwrap());
	// 	println!("Took {} ms", now.elapsed().as_millis());
	// }

	{
		let mut recompress_src = Cursor::new(&decompress_dest);
		let mut recompress_dest = vec![];
		let now = Instant::now();

		let mut reader = xz2::read::XzEncoder::new(&mut recompress_src, 9);
		
		io::copy(&mut reader, &mut recompress_dest)?;

		println!("Recompressed size: {} (xz/LZMA)", recompress_dest.len().file_size(humansize::file_size_opts::DECIMAL).unwrap());
		*total_recompressed_size += recompress_dest.len() as u64;
		println!("Took {} ms", now.elapsed().as_millis());
	}

	Ok(())
}

enum SizeNode {
	Leaf(usize),
	Parent(usize, BTreeMap<String, SizeNode>)
}

impl SizeNode {
	fn add_size(&mut self, size: usize) {
		match self {
			SizeNode::Leaf(ref mut curr_size) => *curr_size += size,
			SizeNode::Parent(ref mut curr_size, ..) => *curr_size += size
		}
	}

	fn add_child(&mut self, name: &str, size: usize) -> &mut SizeNode {
		match self {
			SizeNode::Leaf(this_size) => {
				*self = SizeNode::Parent(*this_size, BTreeMap::new());
				self.add_child(name, size)
			},
			SizeNode::Parent(_size, children) => {
				let child = children.entry(name.to_owned()).or_insert(SizeNode::Leaf(0));
				child.add_size(size);
				child
			}
		}
	}

	fn get_size(&self) -> usize {
		match self {
			SizeNode::Leaf(size) => *size,
			SizeNode::Parent(size, children) => *size
		}
	}
}

fn build_size_tree_children(nbt: &Value, node: &mut SizeNode) {
	if let Value::Compound(map) = nbt {
		for pair in map {
			let leaf = node.add_child(pair.0.as_str(), pair.1.len_bytes());
			build_size_tree_children(pair.1, leaf);
		}
	}
	// TODO: handle lists?
}

fn blockstates_stats(data: &Vec<i64>, palette_length: usize, counts: &mut [u64; 1024]) {
	let num_bits = match (palette_length as f64).log2().ceil() as usize {
		0..=4 => 4,
		x => x,
	};

	let mut local_counts = [0u64; 1024];

	// TODO: note this only works with 1.16!!!
	for value in PackedIntegerArrayIter::new(data.iter(), num_bits as u8)
		.map(|value| value as usize)
		.inspect(|value| assert!(*value < palette_length, "Invalid palette value")) {
		local_counts[value] = local_counts[value] + 1;
	}

	local_counts.sort();
	local_counts.reverse();
	for (i, count) in counts.iter_mut().enumerate() {
		*count += local_counts[i];
	}
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