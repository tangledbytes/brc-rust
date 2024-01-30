use std::{
    collections::HashMap,
    env,
    ffi::{c_int, c_void},
    fs,
    os::fd::AsRawFd,
    slice,
};

const DATASET_SIZE: usize = 512;

extern "C" {
    pub fn mmap(
        addr: *mut c_void,
        len: u64,
        prot: c_int,
        flags: c_int,
        fd: c_int,
        offset: u64,
    ) -> *mut c_void;
}

#[derive(Debug)]
struct Data {
    min: f32,
    max: f32,
    sum: f32,
    count: u32,
}

fn load_file(filename: &str) -> &'static [u8] {
    const PROT_READ: i32 = 0x1;
    const MAP_PRIVATE: i32 = 0x2;

    let file = fs::File::open(filename).expect("failed to open file");
    let size = file
        .metadata()
        .expect("failed to get metadata of the file")
        .len();

    let res = unsafe {
        mmap(
            core::ptr::null_mut(),
            size,
            PROT_READ,
            MAP_PRIVATE,
            file.as_raw_fd(),
            0,
        )
    };

    unsafe { slice::from_raw_parts(res as *const _ as *const u8, size as _) }
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

fn byte_to_float(byt: &[u8]) -> f32 {
    let is_neg = byt[0] == b'-';
    let mut dec_pos = -1;
    let mut num = 0.0;

    for (idx, ch) in byt.iter().enumerate() {
        if idx == 0 && is_neg {
            continue;
        }
        if *ch == b'.' {
            dec_pos = idx as i32;
            continue;
        }

        let digit = (*ch - b'0') as f32;
        num = (num * 10.0) + digit;
    }
    if dec_pos != -1 {
        num /= f32::powi(10.0, (byt.len() as i32 - 1) - dec_pos);
    }

    if is_neg {
        num * -1.0
    } else {
        num
    }
}

fn main() {
    let path = env::args()
        .nth(1)
        .expect("Usage: <bin> <path-to-measurements.txt>");

    let mut store: HashMap<&'static str, Data> = HashMap::with_capacity(DATASET_SIZE);

    let data = load_file(&path);

    for line in std::str::from_utf8(data).unwrap().split('\n') {
        if let Some((name, val)) = line.trim().split_once(';') {
            let val = byte_to_float(val.as_bytes());

            if let Some(data) = store.get_mut(name) {
                data.min = data.min.min(val);
                data.max = data.max.max(val);

                data.sum += val;
                data.count += 1;
            } else {
                store.insert(
                    name,
                    Data {
                        min: val,
                        max: val,
                        sum: val,
                        count: 1,
                    },
                );
            }
        }
    }

    let mut v = store.into_iter().collect::<Vec<_>>();
    v.sort_unstable_by_key(|p| p.0);

    print_store(&v);
}
