use nbt::Value;
use std::collections::BTreeMap;

pub struct NBTStats {
	map: BTreeMap<String, u32>
}

impl NBTStats {
	pub fn new() -> NBTStats {
		NBTStats{ map: BTreeMap::new() }
	}

	pub fn accumulate(&mut self, data: &Value) {
		self.accumulate_internal(data, "".to_string());
	}

	fn accumulate_internal(&mut self, data: &Value, curr_path: String) {
		match data {
			Value::Byte(_) => *self.map.entry(curr_path).or_insert(0) += 1,
			Value::Short(_) => *self.map.entry(curr_path).or_insert(0) += 1,
			Value::Int(_) => *self.map.entry(curr_path).or_insert(0) += 1,
			Value::Long(_) => *self.map.entry(curr_path).or_insert(0) += 1,
			Value::Float(_) => *self.map.entry(curr_path).or_insert(0) += 1,
			Value::Double(_) => *self.map.entry(curr_path).or_insert(0) += 1,
			Value::ByteArray(_) => *self.map.entry(curr_path).or_insert(0) += 1,
			Value::String(_) => *self.map.entry(curr_path).or_insert(0) += 1,
			Value::List(contents) => {
				for value in contents {
					self.accumulate_internal(value, curr_path.clone())
				}
			},
			Value::Compound(contents) => {
				for value in contents {
					match value.1 {
						Value::List(_) => self.accumulate_internal(value.1, curr_path.clone() + value.0 + "[] -> "),
						_ => self.accumulate_internal(value.1, curr_path.clone() + value.0 + " -> ")
					}
				
				}
			},
			Value::IntArray(_) => *self.map.entry(curr_path).or_insert(0) += 1,
			Value::LongArray(_) => *self.map.entry(curr_path).or_insert(0) += 1,
		}
	}

	pub fn print(&self) {
		for value in &self.map {
			println!("{}{}", value.0, value.1);
		}
	}
}