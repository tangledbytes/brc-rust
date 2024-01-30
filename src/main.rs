use std::{collections::HashMap, env, io::{BufRead, BufReader, Read}};

#[derive(Debug)]
struct Data {
    min: f32,
    max: f32,
    sum: f32,
    count: u32,
}

fn print_store(sorted_store: &Vec<(&str, Data)>) {
    print!("{{");

    for (idx, val) in sorted_store.iter().enumerate() {
        print!(
            "{}={:.1}/{:.1}/{:.1}",
            val.0,
            val.1.min,
            val.1.sum / (val.1.count as f32),
            val.1.max
        );
        if idx != sorted_store.len() - 1 {
            print!(", ")
        }
    }

    print!("}}");
}

fn main() {
    let path = env::args().nth(1).expect("Usage: <bin> <path-to-measurements.txt>");
    let mut store: HashMap<&str, Data> = HashMap::new();

    let data = std::fs::read_to_string(path).expect("failed to read file into memory");

    for line in data.trim().split('\n') {
        let (name, val) = line.split_once(';').unwrap();
        let val = val.parse::<f32>().unwrap();

        if let Some(data) = store.get_mut(name) {
            data.min = data.min.min(val);
            data.max = data.max.max(val);

            data.sum += val;
            data.count += 1;
        } else {
            store.insert(name, Data {
                min: val,
                max: val,
                sum: val,
                count: 1,
            });
        }
    }

    let mut v = store.into_iter().collect::<Vec<_>>();
    v.sort_unstable_by_key(|p| p.0);
    print_store(&v);
}