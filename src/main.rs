#![feature(drain_filter)]

use arrayvec::ArrayVec;
use flate2::bufread::ZlibDecoder;
use byteorder::{BigEndian, ReadBytesExt, ByteOrder};
use io::{BufWriter, Seek, SeekFrom, Read, Cursor, Write};
use nbt::{Blob, Value};
use std::{
	fs::File,
	io::{self, BufReader},
	path::Path,
time::Instant, collections::BTreeMap, collections::HashMap};
use humansize::FileSize;

mod util;
mod bytecompressors;
mod integercoders;
mod integertransformers;

use bytecompressors::ByteCompressor;
use integercoders::IntegerCoder;
use integertransformers::IntegerTransformer;

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
	for file in std::fs::read_dir(Path::new("bench"))? {
		let file = file?;
		println!("Reading file {:?}", &file.path());
		fn bench_3<Transformer: IntegerTransformer, Coder: IntegerCoder>(orig_path: &Path) -> anyhow::Result<()> {
			println!("\t\tCompressor: None");
			benchmark_file::<Transformer, Coder, bytecompressors::None>(orig_path)?;
			// println!("\t\tCompressor: LZMA");
			// benchmark_file::<Transformer, Coder, bytecompressors::LZMA>(orig_path)?;
			println!("\t\tCompressor: Zlib");
			benchmark_file::<Transformer, Coder, bytecompressors::Zlib>(orig_path)?;
			Ok(())
		}
		fn bench_2<Transformer: IntegerTransformer>(orig_path: &Path) -> anyhow::Result<()> {
			println!("\tCoder: Arithmetic");
			bench_3::<Transformer, integercoders::ArithmeticCoding>(orig_path)?;
			println!("\tCoder: Bytewise");
			bench_3::<Transformer, integercoders::Bytewise>(orig_path)?;
			Ok(())
		}
		println!("Transformer: None");
		bench_2::<integertransformers::None>(&file.path())?;
		// println!("Transformer: Delta of prev value");
		// bench_2::<integertransformers::DeltaLeft>(&file.path())?;
		println!("Transformer: Move-to-front");
		bench_2::<integertransformers::MoveToFront>(&file.path())?;
		println!("Transformer: Move-to-front with 16/256 lookbehind");
		bench_2::<integertransformers::MoveToFrontLookbehind>(&file.path())?;
	}

	Ok(())
}

fn benchmark_file<Transformer: IntegerTransformer, Coder: IntegerCoder, Compressor: ByteCompressor>(orig_path: &Path) -> anyhow::Result<()> {
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

	//println!("Found {} chunks", chunk_positions.len());

	let orig_size = buf_reader.seek(SeekFrom::End(0))?;
	//println!("Original size: {}", orig_size.file_size(humansize::file_size_opts::DECIMAL).unwrap());
	let mut unpadded_size = 0;
	let mut decompressed_size = 0;

	let mut final_size = 0;
	let mut palette_sizes_map: BTreeMap<u32, u64> = BTreeMap::new();

	for pos in &chunk_positions {
		buf_reader.seek(SeekFrom::Start((pos.offset * 4096) as u64))?;
		let _length = buf_reader.read_u32::<BigEndian>()?;
		let _compression_type = buf_reader.read_u8()?;
		// TODO: handle non-zlib

		let mut decoder = ZlibDecoder::new(&mut buf_reader);
		let chunk_data = Blob::from_reader(&mut decoder)?;

		unpadded_size += decoder.total_in();
		decompressed_size += decoder.total_out();

		if let Some(value) = chunk_data.get("Level") {
			let mut value = value.clone();
			if let Value::Compound(ref mut level_map) = value {
				if let Some(Value::List(sections)) = level_map.get_mut("Sections") {
					for section in sections {
						if let Value::Compound(section) = section {
							if let Some(Value::List(palette)) = section.get("Palette") {
								let palette_length = palette.len();
								if let Some(Value::LongArray(data)) = section.get("BlockStates") {
									run_tests::<Transformer, Coder, Compressor>(data, palette_length as u32, &mut final_size, &mut palette_sizes_map)?;
								}
							}
						}
					}
				}
			}
		}

	}
	
	//println!("Unpadded size: {}", unpadded_size.file_size(humansize::file_size_opts::DECIMAL).unwrap());
	//println!("Decompressed size: {}", decompressed_size.file_size(humansize::file_size_opts::DECIMAL).unwrap());
	println!("\t\tBlockstates final size: {}", final_size.file_size(humansize::file_size_opts::DECIMAL).unwrap());
	println!("\t\tPalette length / size distribution: ");
	for v in palette_sizes_map {
		println!("\t\t\t{}, {}" , v.0, v.1);
	}

	Ok(())
}

fn run_tests<Transformer: IntegerTransformer, Coder: IntegerCoder, Compressor: ByteCompressor>(data: &Vec<i64>, palette_length: u32, final_size: &mut i64, palette_sizes_map: &mut BTreeMap<u32, u64>) -> anyhow::Result<()> {
	if palette_length <= 1 {
		return Ok(());
	}
	
	let num_bits = match (palette_length as f64).log2().ceil() as usize {
		0..=4 => 4,
		x => x,
	};

	let decoded_data: ArrayVec<u32, 4096> = PackedIntegerArrayIter::new(data.iter(), num_bits as u8)
		//.map(|value| value as usize)
		.inspect(|value| assert!(*value < palette_length, "Invalid palette value"))
		.take(4096)
		.collect();
	let mut arr = decoded_data.into_inner().unwrap();
	let mut palette_size_transformed = palette_length;
	// let arr_orig = arr.clone();
	
	Transformer::transform(&mut arr, &mut palette_size_transformed);

	// let arr_transformed = arr.clone();

	// Transformer::reverse(&mut arr, &mut palette_size_transformed);
	// if !arr.eq(&arr_orig) {
	// 	println!("{:?}", arr_orig);
	// 	println!("{:?}", arr_transformed);
	// 	println!("{:?}", arr);
	// 	panic!("Oh no!");
	// }

	let mut encoded = vec![];
	Coder::encode(&arr, &mut encoded, palette_size_transformed);

	let mut compressed = vec![];
	Compressor::compress(&encoded, &mut compressed);

	*final_size += compressed.len() as i64;
	*palette_sizes_map.entry(palette_length).or_insert(0) += compressed.len() as u64;

	Ok(())
}