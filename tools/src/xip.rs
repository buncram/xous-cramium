mod bootconfig;
mod phase1;
mod phase2;
mod minielf;
mod args;

use core::num::NonZeroUsize;
use core::{mem, ptr, slice};

pub type XousPid = u8;
pub const PAGE_SIZE: usize = 4096;
pub const WORD_SIZE: usize = mem::size_of::<usize>();

pub const USER_STACK_TOP: usize = 0x8000_0000;
pub const PAGE_TABLE_OFFSET: usize = 0xff40_0000;
pub const PAGE_TABLE_ROOT_OFFSET: usize = 0xff80_0000;
pub const CONTEXT_OFFSET: usize = 0xff80_1000;
pub const USER_AREA_END: usize = 0xff00_0000;

// All of the kernel structures must live within Megapage 1023,
// and therefore are limited to 4 MB.
pub const EXCEPTION_STACK_TOP: usize = 0xffff_0000;
pub const KERNEL_LOAD_OFFSET: usize = 0xffd0_0000;
pub const KERNEL_STACK_PAGE_COUNT: usize = 1;
pub const KERNEL_ARGUMENT_OFFSET: usize = 0xffc0_0000;
pub const GUARD_MEMORY_BYTES: usize = 2 * PAGE_SIZE;

pub const FLG_VALID: usize = 0x1;
pub const FLG_X: usize = 0x8;
pub const FLG_W: usize = 0x4;
pub const FLG_R: usize = 0x2;
pub const FLG_U: usize = 0x10;
pub const FLG_A: usize = 0x40;
pub const FLG_D: usize = 0x80;
pub const STACK_PAGE_COUNT: usize = 8;

pub const VDBG: bool = true;

#[repr(C)]
struct MemoryRegionExtra {
    start: u32,
    length: u32,
    name: u32,
    padding: u32,
}

/// A single RISC-V page table entry.  In order to resolve an address,
/// we need two entries: the top level, followed by the lower level.
#[repr(C)]
pub struct PageTable {
    entries: [usize; PAGE_SIZE / WORD_SIZE],
}

#[repr(C)]
pub struct InitialProcess {
    /// The RISC-V SATP value, which includes the offset of the root page
    /// table plus the process ID.
    satp: usize,

    /// Where execution begins
    entrypoint: usize,

    /// Address of the top of the stack
    sp: usize,
}

/// This describes the kernel as well as initially-loaded processes
#[repr(C)]
pub struct ProgramDescription {
    /// Physical source address of this program in RAM (i.e. SPI flash).
    /// The image is assumed to contain a text section followed immediately
    /// by a data section.
    load_offset: u32,

    /// Start of the virtual address where the .text section will go.
    /// This section will be marked non-writable, executable.
    text_offset: u32,

    /// How many bytes of data to load from the source to the target
    text_size: u32,

    /// Start of the virtual address of .data and .bss section in RAM.
    /// This will simply allocate this memory and mark it "read-write"
    /// without actually copying any data.
    data_offset: u32,

    /// Size of the .data section, in bytes..  This many bytes will
    /// be allocated for the data section.
    data_size: u32,

    /// Size of the .bss section, in bytes.
    bss_size: u32,

    /// Virtual address entry point.
    entrypoint: u32,
}


/// Currently, all processes originate from FLASH, and are copied into RAM:
///   - Page table entries are set up
///   - RAM is marked as allocated during the copy
///   - Permissions are set on the copied pages
///
/// The used up RAM pages are passed to the kernel in the runtime page tracker.
///
/// This function receives the following parameters:
///   - A process, described as a MiniElf file
///   - The target physical address of the MiniElf file in FLASH
///
/// The function will generate the following artifacts:
///   - A list of FLASH page table entries to populate (as MapMemory commands)
///   - A list of memory blocks to copy & allocate
///
/// The loader will parse these lists and call MapMemory to put the FLASH
/// in the correct locations, and execute the memory block copies while
/// updating the runtime page tracker.
pub fn layout_process() {

}

pub fn print_pagetable(root: usize) {
    println!(
        "Memory Maps (SATP: {:08x}  Root: {:08x}):",
        root,
        root << 12
    );
    let l1_pt = unsafe { &mut (*((root << 12) as *mut PageTable)) };
    for (i, l1_entry) in l1_pt.entries.iter().enumerate() {
        if *l1_entry == 0 {
            continue;
        }
        let _superpage_addr = i as u32 * (1 << 22);
        println!(
            "    {:4} Superpage for {:08x} @ {:08x} (flags: {:02x})",
            i,
            _superpage_addr,
            (*l1_entry >> 10) << 12,
            l1_entry & 0xff
        );
        // let l0_pt_addr = ((l1_entry >> 10) << 12) as *const u32;
        let l0_pt = unsafe { &mut (*(((*l1_entry >> 10) << 12) as *mut PageTable)) };
        for (j, l0_entry) in l0_pt.entries.iter().enumerate() {
            if *l0_entry == 0 {
                continue;
            }
            let _page_addr = j as u32 * (1 << 12);
            println!(
                "        {:4} {:08x} -> {:08x} (flags: {:02x})",
                j,
                _superpage_addr + _page_addr,
                (*l0_entry >> 10) << 12,
                l0_entry & 0xff
            );
        }
    }
}

pub unsafe fn bzero<T>(mut sbss: *mut T, ebss: *mut T)
where
    T: Copy,
{
    if VDBG {println!("ZERO: {:08x} - {:08x}", sbss as usize, ebss as usize);}
    while sbss < ebss {
        // NOTE(volatile) to prevent this from being transformed into `memclr`
        ptr::write_volatile(sbss, mem::zeroed());
        sbss = sbss.offset(1);
    }
}

/// Copy _count_ **bytes** from src to dest.
pub unsafe fn memcpy<T>(dest: *mut T, src: *const T, count: usize)
where
    T: Copy,
{
    if VDBG {println!(
        "COPY: {:08x} - {:08x} {} {:08x} - {:08x}",
        src as usize,
        src as usize + count,
        count,
        dest as usize,
        dest as usize + count
    );}
    let mut offset = 0;
    while offset < (count / mem::size_of::<T>()) {
        dest.add(offset)
            .write_volatile(src.add(offset).read_volatile());
        offset += 1
    }
}