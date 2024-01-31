use std::{
    ffi::{c_int, c_void},
    fs,
    os::fd::AsRawFd,
    slice, thread,
};

mod util;

const MAP_SIZE: usize = 7599;

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
    min: i32,
    max: i32,
    sum: i32,
    count: u32,
}

struct LookupTable {
    slots: Box<[Option<(&'static [u8], Data)>; MAP_SIZE]>,
}

impl LookupTable {
    const SLOT_DEFAULT_VALUE: Option<(&'static [u8], Data)> = None;

    fn new() -> Self {
        LookupTable {
            slots: Box::new([Self::SLOT_DEFAULT_VALUE; MAP_SIZE]),
        }
    }

    fn insert_with_hash(&mut self, k: &'static [u8], v: Data, hash: u32) {
        let slot_idx = (hash as usize) % (MAP_SIZE);

        if let Some(slot) = unsafe { self.slots.get_unchecked_mut(slot_idx) } {
            slot.1 = v;
        } else {
            unsafe {
                *self.slots.get_unchecked_mut(slot_idx) = Some((k, v));
            }
        }
    }

    fn get_mut_with_hash(&mut self, k: &'static [u8], hash: u32) -> Option<&mut Data> {
        let slot_idx = (hash as usize) % (MAP_SIZE);

        if let Some(slot) = unsafe { self.slots.get_unchecked_mut(slot_idx) } {
            Some(&mut slot.1)
        } else {
            None
        }
    }
}

impl IntoIterator for LookupTable {
    type Item = (&'static [u8], Data, u32);

    type IntoIter = MapIter;

    fn into_iter(self) -> Self::IntoIter {
        MapIter { idx: 0, map: self }
    }
}

struct MapIter {
    idx: usize,
    map: LookupTable,
}

impl Iterator for MapIter {
    type Item = (&'static [u8], Data, u32);

    fn next(&mut self) -> Option<Self::Item> {
        for idx in self.idx..MAP_SIZE {
            if let Some((k, v)) = unsafe { self.map.slots.get_unchecked_mut(idx).take() } {
                return Some((k, v, idx as u32));
            }
        }

        None
    }
}

struct ParseResult {
    place: &'static [u8],
    place_hash: u32,
    val: i32,
    next: usize,
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

fn cluster_process(filename: &str, store: &mut LookupTable) {
    let cpus = thread::available_parallelism().unwrap().get() as u64;
    let mut stores: Vec<LookupTable> = Vec::with_capacity(cpus as usize);
    for _ in 0..cpus {
        stores.push(LookupTable::new());
    }

    let file = fs::File::open(filename).expect("failed to open file");
    let size = file
        .metadata()
        .expect("failed to get metadata of the file")
        .len();
    let data_size = size;
    let size_per_cpu = data_size / cpus;
    let remains = data_size % cpus;

    thread::scope(|s| {
        for (idx, store) in stores.iter_mut().enumerate() {
            let itr_remainder = remains;

            s.spawn(move || {
                // Pin thread to a CPU
                util::set_cpu_affinity(idx);

                let mut size = size_per_cpu;
                let idx = idx as u64;
                if idx == cpus - 1 {
                    size += itr_remainder;
                }

                let data = load_file(filename);
                consume(data, (idx * size_per_cpu) as _, size as _, store);
            });
        }
    });

    for local_store in stores {
        for (k, v, hash) in local_store {
            if let Some(data) = store.get_mut_with_hash(k, hash) {
                data.min = data.min.min(v.min);
                data.max = data.max.max(v.max);
                data.sum += v.sum;
                data.count += v.count;
            } else {
                store.insert_with_hash(
                    k,
                    Data {
                        min: v.min,
                        max: v.max,
                        sum: v.sum,
                        count: v.count,
                    },
                    hash,
                );
            }
        }
    }
}

fn consume(data: &'static [u8], mut chunk_offset: usize, size: usize, store: &mut LookupTable) {
    // 1. Find the start point
    let start: usize;
    if chunk_offset == 0 {
        start = 0;
    } else {
        loop {
            if data[chunk_offset - 1] == b'\n' {
                start = chunk_offset as _;
                break;
            }

            chunk_offset += 1;
        }
    }

    // 2. Parse the data
    let mut readptr = start;
    while readptr - start < size {
        if let Some(end) = process(data, readptr, store) {
            readptr = end + 1;
        } else {
            break;
        }
    }
}

fn process(data: &'static [u8], offset: usize, store: &mut LookupTable) -> Option<usize> {
    if let Some(parsed) = parse_line(data, offset) {
        if let Some(data) = store.get_mut_with_hash(parsed.place, parsed.place_hash) {
            data.min = data.min.min(parsed.val);
            data.max = data.max.max(parsed.val);
            data.sum += parsed.val;
            data.count += 1;
        } else {
            store.insert_with_hash(
                parsed.place,
                Data {
                    min: parsed.val,
                    max: parsed.val,
                    sum: parsed.val,
                    count: 1,
                },
                parsed.place_hash,
            );
        }

        Some(parsed.next)
    } else {
        None
    }
}

fn parse_line(data: &'static [u8], offset: usize) -> Option<ParseResult> {
    if offset >= data.len() {
        return None;
    }

    let mut delim = offset;

    let mut loc_hash: u32 = 5381;
    let mut loc: &[u8] = unsafe { data.get_unchecked(offset..delim) }; // useless init

    let mut idx = offset;

    // Find the delimiter and compute hash till that point
    while idx < data.len() {
        let ch = unsafe { *data.get_unchecked(idx) };
        if ch == b';' {
            delim = idx;
            loc = unsafe { data.get_unchecked(offset..delim) };

            break;
        }

        loc_hash = ch as u32 + (loc_hash << 6) + (loc_hash << 16) - loc_hash;

        idx += 1;
    }

    // Skip past delimiter
    idx += 1;

    let mut val: i32;
    let mut ch = unsafe { *data.get_unchecked(idx) };
    let isneg = if ch == b'-' {
        idx += 1;
        true
    } else {
        false
    };

    // Parse the float and find new line (not really, assume that there is just one f64 and then '\n')
    // Assuming the structure can be either:
    // 1. ab.c\n
    // 2. b.c\n
    ch = unsafe { *data.get_unchecked(idx) };

    val = ch as i32;
    val *= 10;

    idx += 1;
    ch = unsafe { *data.get_unchecked(idx) };

    if ch == b'.' {
        idx += 1;
        ch = unsafe { *data.get_unchecked(idx) };

        val += ch as i32;

        if isneg { val = -val; }

        return Some(ParseResult {
            place: loc,
            place_hash: loc_hash,
            val,
            next: idx + 1,
        });
    }

    val += ch as i32;
    val *= 10;

    // Assume that the next character will be a decimal
    idx += 1 + 1;
    ch = unsafe { *data.get_unchecked(idx) };

    val += ch as i32;

    if isneg { val = -val; }

    Some(ParseResult {
        place: loc,
        place_hash: loc_hash,
        val,
        next: idx + 1,
    })
}

fn print_store(sorted_store: &Vec<(&[u8], Data, u32)>) {
    print!("{{");

    for (idx, val) in sorted_store.iter().enumerate() {
        print!(
            "{}={:.1}/{:.1}/{:.1}",
            unsafe { std::str::from_utf8_unchecked(val.0) },
            conv_num(val.1.min),
            conv_num(val.1.sum) / (val.1.count as f32),
            conv_num(val.1.max),
        );
        if idx != sorted_store.len() - 1 {
            print!(", ")
        }
    }

    print!("}}");
}

fn conv_num(mut num: i32) -> f32 {
    let thrice = 111 * b'0' as i32;
    let twice = 11 * b'0' as i32;

    let divisor = if num < 0 {
        num = -num;
        -10.
    } else {
        10.
    };

    if num > thrice {
        return (num - thrice) as f32 / divisor;
    }
    if num > twice {
        return (num - twice) as f32 / divisor;
    }

    panic!("number shouldn't be this small!")
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("Usage: <bin> <path-to-measurements.txt>");

    let mut store = LookupTable::new();

    cluster_process(&path, &mut store);

    let mut v = store.into_iter().collect::<Vec<_>>();
    v.sort_unstable_by_key(|p| p.0);

    print_store(&v);
}
