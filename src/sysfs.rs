use std::fs::File;
use std::io::prelude::*;
use std::path::Path;

const DEBUG: bool = false;

pub fn try_write<P: AsRef<Path>>(path: P, value: &'_ str) -> Result<(), std::io::Error> {

    let value_with_newline = format!("{}\n", value);

    match std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .create_new(false)
        .open(path.as_ref())
    {
        Ok(mut file) => {
            match file.write_all(value_with_newline.as_bytes()) {
                Ok(_) => file.sync_all(),
                Err(err) => Err(err)
            }
        }
        Err(err) => Err(err)
    }
}


pub fn write<P: AsRef<Path>>(path: P, value: &'_ str) {

    if DEBUG {
        let value_with_newline = format!("{}\n", value);
        let path_str = path.as_ref().to_str().unwrap();

        println!("Writing: {} -> {}", value_with_newline, path_str);
    }
    try_write(path, value).expect("Failed to write file");
}

pub fn parse_string_from_file<T: std::str::FromStr, P: AsRef<Path>>(path: &P) -> T {
    let data: String = read_string_from_file(path);

    match data.trim().parse::<T>() {
        Ok(parsed) => parsed,
        Err(_) => panic!("Could not parse {}", data)
    }
}

pub fn try_read_string_from_file<P: AsRef<Path>>(path: &P) -> Option<String> {
    let mut data = String::new();
    File::open(path)
        .and_then(|mut file| file.read_to_string(&mut data))
        .map_or(None, |_| Some(data))
}

pub fn read_string_from_file<P: AsRef<Path>>(path: &P) -> String {
    let mut file = File::open(path).expect("Could not open file");
    let mut data = String::new();
    file.read_to_string(&mut data).expect("Could not read from file");

    data
}
