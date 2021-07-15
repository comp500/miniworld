use std::io::Cursor;

use flate2::{Compression, read::{ZlibDecoder, ZlibEncoder}};

pub trait ByteCompressor {
	fn compress(data: &Vec<u8>, dest: &mut Vec<u8>);
	fn decompress(data: &Vec<u8>, dest: &mut Vec<u8>);
}

pub struct None;

impl ByteCompressor for None {
    fn compress(data: &Vec<u8>, dest: &mut Vec<u8>) {
        dest.extend_from_slice(data)
    }

    fn decompress(data: &Vec<u8>, dest: &mut Vec<u8>) {
        dest.extend_from_slice(data)
    }
}

pub struct LZMA;

impl ByteCompressor for LZMA {
    fn compress(data: &Vec<u8>, dest: &mut Vec<u8>) {
		let mut cursor = Cursor::new(data);
        let mut reader = xz2::read::XzEncoder::new(&mut cursor, 9);
		std::io::copy(&mut reader, dest).unwrap();
    }

    fn decompress(data: &Vec<u8>, dest: &mut Vec<u8>) {
        let mut cursor = Cursor::new(data);
        let mut reader = xz2::read::XzDecoder::new(&mut cursor);
		std::io::copy(&mut reader, dest).unwrap();
    }
}

pub struct Zlib;

impl ByteCompressor for Zlib {
    fn compress(data: &Vec<u8>, dest: &mut Vec<u8>) {
		let mut cursor = Cursor::new(data);
        let mut reader = ZlibEncoder::new(&mut cursor, Compression::best());
		std::io::copy(&mut reader, dest).unwrap();
    }

    fn decompress(data: &Vec<u8>, dest: &mut Vec<u8>) {
        let mut cursor = Cursor::new(data);
        let mut reader = ZlibDecoder::new(&mut cursor);
		std::io::copy(&mut reader, dest).unwrap();
    }
}