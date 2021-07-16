use std::io::Cursor;

use arcode::bitbit::BitWriter;
use arcode::decode::decoder::ArithmeticDecoder;
use arcode::encode::encoder::ArithmeticEncoder;
use arcode::util::source_model::SourceModel;
use arcode::util::source_model_builder::{EOFKind, SourceModelBuilder};
use bitbit::{BitReader, MSB};

pub trait IntegerCoder {
	fn encode(data: &[u32; 4096], dest: &mut Vec<u8>, palette_size: u32);
	fn decode(data: &[u8], dest: &mut[u32; 4096], palette_size: u32);
}

pub struct ArithmeticCoding;

fn build_model(palette_size: u32) -> SourceModel {
	SourceModelBuilder::new().num_symbols(palette_size).eof(EOFKind::None).build()
}

impl IntegerCoder for ArithmeticCoding {
    fn encode(data: &[u32; 4096], dest: &mut Vec<u8>, palette_size: u32) {
		let mut model = build_model(palette_size);
		
		let mut compressed_writer = BitWriter::new(Cursor::new(dest));
		let mut encoder = ArithmeticEncoder::new(32);

		for &sym in data {
			encoder.encode(sym, &model, &mut compressed_writer).unwrap();
			model.update_symbol(sym);
		}

		//encoder.encode(model.eof(), &model, &mut compressed_writer).unwrap();
		encoder.finish_encode(&mut compressed_writer).unwrap();
		compressed_writer.pad_to_byte().unwrap();
    }

    fn decode(data: &[u8], dest: &mut[u32; 4096], palette_size: u32) {
        let mut model = build_model(palette_size);

		let mut compressed_reader = BitReader::<_, MSB>::new(Cursor::new(data));
		let mut decoder = ArithmeticDecoder::new(32);

		for i in 0..4096 {
			let sym = decoder.decode(&model, &mut compressed_reader).unwrap();
			model.update_symbol(sym);
			dest[i] = sym;
		}
    }
}

struct PackedIntegers; // TODO

struct Simple16; // TODO

pub struct Bytewise;

impl IntegerCoder for Bytewise {
    fn encode(data: &[u32; 4096], dest: &mut Vec<u8>, _palette_size: u32) {
        for v in data {
			dest.push(*v as u8);
		}
    }

    fn decode(data: &[u8], dest: &mut[u32; 4096], _palette_size: u32) {
        for i in 0..4096 {
			dest[i] = data[i] as u32;
		}
    }
}