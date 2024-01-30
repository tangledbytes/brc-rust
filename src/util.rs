use std::ffi::c_int;

#[repr(C)]
struct cpu_set_t {
    #[cfg(all(target_pointer_width = "32", not(target_arch = "x86_64")))]
    bits: [u32; 32],
    #[cfg(not(all(target_pointer_width = "32", not(target_arch = "x86_64"))))]
    bits: [u64; 16],
}

extern "C" {
    fn sched_setaffinity(pid: i32, cpusetsize: usize, cpuset: *const cpu_set_t) -> c_int;
}

fn CPU_SET(cpu: usize, cpuset: &mut cpu_set_t) {
    let size_in_bits = 8 * std::mem::size_of_val(&cpuset.bits[0]); // 32, 64 etc
    let (idx, offset) = (cpu / size_in_bits, cpu % size_in_bits);
    cpuset.bits[idx] |= 1 << offset;
}

#[cfg(target_os = "linux")]
#[inline(always)]
pub fn set_cpu_affinity(id: usize) -> bool {
    let mut cpuset = unsafe { std::mem::zeroed::<cpu_set_t>() };

    unsafe { CPU_SET(id, &mut cpuset) };

    let res = unsafe { sched_setaffinity(0, std::mem::size_of::<cpu_set_t>(), &cpuset) };

    res == 0
}

#[cfg(target_os = "macos")]
#[inline(always)]
pub fn set_cpu_affinity(id: usize) -> bool {
    false
}
