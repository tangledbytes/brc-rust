use std::{
    ffi::{c_int, c_void},
    fs,
    os::fd::AsRawFd,
    slice, thread,
};

const MAP_SIZE: usize = 10829;

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

struct Map {
    slots1: Box<[Option<u32>; MAP_SIZE]>,
    slots2: Box<[Option<(&'static [u8], Data)>; MAP_SIZE]>,
}

impl Map {
    const SLOT1_DEFAULT_VALUE: Option<u32> = None;
    const SLOT2_DEFAULT_VALUE: Option<(&'static [u8], Data)> = None;

    fn new() -> Self {
        Map {
            slots1: Box::new([Self::SLOT1_DEFAULT_VALUE; MAP_SIZE]),
            slots2: Box::new([Self::SLOT2_DEFAULT_VALUE; MAP_SIZE]),
        }
    }

    fn insert(&mut self, k: &'static [u8], v: Data) {
        let hash = Self::hash(k);
        self.insert_with_hash(k, v, hash)
    }

    fn get_mut(&mut self, k: &'static [u8]) -> Option<&mut Data> {
        let hash = Self::hash(k);
        self.get_mut_with_hash(k, hash)
    }

    fn insert_with_hash(&mut self, k: &'static [u8], v: Data, hash: u32) {
        let slot_idx = (hash as usize) % MAP_SIZE;

        if let Some(slot_hash) = unsafe { *self.slots1.get_unchecked(slot_idx) } {
            let slot2 = unsafe { self.slots2.get_unchecked_mut(slot_idx) }
                .as_mut()
                .expect("should exist if slot1 is filled");
            if slot_hash == hash {
                slot2.1 = v;
                return;
            }

            panic!("unexpected insert collision")
        } else {
            unsafe {
                *self.slots1.get_unchecked_mut(slot_idx) = Some(hash);
            }
            unsafe {
                *self.slots2.get_unchecked_mut(slot_idx) = Some((k, v));
            }
        }
    }

    fn get_mut_with_hash(&mut self, k: &'static [u8], hash: u32) -> Option<&mut Data> {
        let slot_idx = (hash as usize) % MAP_SIZE;

        if let Some(slot_hash) = unsafe { *self.slots1.get_unchecked(slot_idx) } {
            if slot_hash == hash {
                let data = unsafe { self.slots2.get_unchecked_mut(slot_idx) }
                    .as_mut()
                    .expect("should exist if slot1 is filled");
                return Some(&mut data.1);
            }

            panic!("unexpected read collision")
        } else {
            None
        }
    }

    fn hash(k: &[u8]) -> u32 {
        let mut v: u32 = 2166136261;

        for idx in 0..k.len() {
            let ch = unsafe { k.get_unchecked(idx) };

            v ^= *ch as u32;
            v = v.wrapping_mul(16777619);
        }

        v
    }
}

impl IntoIterator for Map {
    type Item = (&'static [u8], Data);

    type IntoIter = MapIter;

    fn into_iter(self) -> Self::IntoIter {
        MapIter { idx: 0, map: self }
    }
}

struct MapIter {
    idx: usize,
    map: Map,
}

impl Iterator for MapIter {
    type Item = (&'static [u8], Data);

    fn next(&mut self) -> Option<Self::Item> {
        for idx in self.idx..MAP_SIZE {
            if self.map.slots1[idx].is_some() {
                self.idx = idx + 1;

                return Some(
                    self.map.slots2[idx]
                        .take()
                        .expect("should exist if slot1 is filled"),
                );
            }
        }

        None
    }
}

struct ParseResult {
    place: &'static [u8],
    place_hash: u32,
    val: f32,
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

fn cluster_process(filename: &str, store: &mut Map) {
    let cpus = thread::available_parallelism().unwrap().get() as u64;
    let mut stores: Vec<Map> = Vec::with_capacity(cpus as usize);
    for _ in 0..cpus {
        stores.push(Map::new());
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
        for (k, v) in local_store {
            if let Some(data) = store.get_mut(k) {
                if v.min < data.min {
                    data.min = v.min;
                }
                if v.max > data.max {
                    data.max = v.max;
                }

                data.sum += v.sum;
                data.count += v.count;
            } else {
                store.insert(
                    k,
                    Data {
                        min: v.min,
                        max: v.max,
                        sum: v.sum,
                        count: v.count,
                    },
                );
            }
        }
    }
}

fn consume(data: &'static [u8], mut chunk_offset: usize, size: usize, store: &mut Map) {
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

fn process(data: &'static [u8], offset: usize, store: &mut Map) -> Option<usize> {
    if let Some(parsed) = parse_line(data, offset) {
        if let Some(data) = store.get_mut_with_hash(parsed.place, parsed.place_hash) {
            if parsed.val < data.min {
                data.min = parsed.val;
            }
            if parsed.val > data.max {
                data.max = parsed.val;
            }

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

    let mut loc_hash: u32 = 2166136261;
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

        loc_hash ^= ch as u32;
        loc_hash = loc_hash.wrapping_mul(16777619);

        idx += 1;
    }

    // Skip past delimiter
    idx += 1;

    let mut val: i32;
    let mut ch = unsafe { *data.get_unchecked(idx) };
    let divisor = if ch == b'-' {
        idx += 1;
        -10.
    } else {
        10.
    };

    // Parse the float and find new line (not really, assume that there is just one f64 and then '\n')
    // Assuming the structure can be either:
    // 1. ab.c\n
    // 2. b.c\n
    ch = unsafe { *data.get_unchecked(idx) };

    val = (ch - b'0') as i32;
    val *= 10;

    idx += 1;
    ch = unsafe { *data.get_unchecked(idx) };

    if ch == b'.' {
        idx += 1;
        ch = unsafe { *data.get_unchecked(idx) };

        val += (ch - b'0') as i32;

        return Some(ParseResult {
            place: loc,
            place_hash: loc_hash,
            val: val as f32 / divisor,
            next: idx + 1,
        });
    }

    val += (ch - b'0') as i32;
    val *= 10;

    // Assume that the next character will be a decimal
    idx += 1 + 1;
    ch = unsafe { *data.get_unchecked(idx) };

    val += (ch - b'0') as i32;

    Some(ParseResult {
        place: loc,
        place_hash: loc_hash,
        val: val as f32 / divisor,
        next: idx + 1,
    })
}

fn print_store(sorted_store: &Vec<(&[u8], Data)>) {
    print!("{{");

    for (idx, val) in sorted_store.iter().enumerate() {
        print!(
            "{}={:.1}/{:.1}/{:.1}",
            unsafe { std::str::from_utf8_unchecked(val.0) },
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
    let path = std::env::args()
        .nth(1)
        .expect("Usage: <bin> <path-to-measurements.txt>");

    let mut store = Map::new();

    cluster_process(&path, &mut store);

    let mut v = store.into_iter().collect::<Vec<_>>();
    v.sort_unstable_by_key(|p| p.0);

    print_store(&v);
}
