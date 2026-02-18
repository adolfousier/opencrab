use phf_codegen::Map;
use serde::Deserialize;
use std::env;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;

#[derive(Deserialize)]
struct Tokens {
    single_byte: Vec<String>,
    double_byte: Vec<Vec<String>>,
}

fn main() {
    println!("cargo:rerun-if-changed=src/tokens.json");
    println!("cargo:rerun-if-changed=build.rs");

    let path = Path::new(&env::var("OUT_DIR").unwrap()).join("token_maps.rs");
    let mut file = BufWriter::new(File::create(&path).unwrap());

    let tokens_json = fs::read_to_string("src/tokens.json").unwrap();
    let tokens: Tokens = serde_json::from_str(&tokens_json).unwrap();

    let mut single_byte_map = Map::new();
    let single_byte_values: Vec<String> = (0..tokens.single_byte.len())
        .map(|i| i.to_string())
        .collect();
    for (i, token) in tokens.single_byte.iter().enumerate() {
        if !token.is_empty() {
            single_byte_map.entry(token.as_str(), &single_byte_values[i]);
        }
    }
    writeln!(
        &mut file,
        "static SINGLE_BYTE_MAP: phf::Map<&'static str, u8> = \n{};",
        single_byte_map.build()
    )
    .unwrap();

    let mut double_byte_map = Map::new();
    let mut double_byte_values = Vec::new();
    for (dict_idx, dict) in tokens.double_byte.iter().enumerate() {
        for (token_idx, _token) in dict.iter().enumerate() {
            let value = format!("({}, {})", dict_idx, token_idx);
            double_byte_values.push(value);
        }
    }

    let mut value_idx = 0;
    for dict in tokens.double_byte.iter() {
        for token in dict.iter() {
            double_byte_map.entry(token.as_str(), &double_byte_values[value_idx]);
            value_idx += 1;
        }
    }
    writeln!(
        &mut file,
        "\nstatic DOUBLE_BYTE_MAP: phf::Map<&'static str, (u8, u8)> = \n{};",
        double_byte_map.build()
    )
    .unwrap();

    writeln!(&mut file, "\nstatic SINGLE_BYTE_TOKENS: &[&str] = &[").unwrap();
    for token in &tokens.single_byte {
        writeln!(&mut file, "    {:?},", token).unwrap();
    }
    writeln!(&mut file, "];").unwrap();

    writeln!(&mut file, "\nstatic DOUBLE_BYTE_TOKENS: &[&[&str]] = &[").unwrap();
    for dict in &tokens.double_byte {
        writeln!(&mut file, "    &[").unwrap();
        for token in dict {
            writeln!(&mut file, "        {:?},", token).unwrap();
        }
        writeln!(&mut file, "    ],").unwrap();
    }
    writeln!(&mut file, "];").unwrap();
}
