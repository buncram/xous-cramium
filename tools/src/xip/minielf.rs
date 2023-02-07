use crate::xip::*;
use crate::xip::bootconfig::BootConfig;
use crate::xip::args::KernelArgument;

#[repr(C)]
pub struct MiniElfSection {
    // Virtual address of this section
    pub virt: u32,

    // A combination of the size and flags
    size_and_flags: u32,
}

impl MiniElfSection {
    pub fn len(&self) -> usize {
        // Strip off the top four bits, which contain the flags.
        let len = self.size_and_flags & !0xff00_0000;
        len as usize
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    pub fn flags(&self) -> usize {
        let le_bytes = self.size_and_flags >> 24;
        le_bytes as usize
    }

    pub fn no_copy(&self) -> bool {
        self.size_and_flags & (1 << 25) != 0
    }
}

/// Describes a Mini ELF file, suitable for loading into RAM
pub struct MiniElf {
    /// Physical source address of this program in RAM (i.e. SPI flash).
    pub load_offset: u32,

    /// Virtual address of the entrypoint
    pub entry_point: u32,

    /// All of the sections inside this file
    pub sections: &'static [MiniElfSection],
}

impl MiniElf {
    pub fn new(tag: &KernelArgument) -> Self {
        let ptr = tag.data.as_ptr();
        unsafe {
            MiniElf {
                load_offset: ptr.add(0).read(),
                entry_point: ptr.add(1).read(),
                sections: slice::from_raw_parts(
                    ptr.add(2) as *mut MiniElfSection,
                    (tag.size as usize - 8) / mem::size_of::<MiniElfSection>(),
                ),
            }
        }
    }

    /// Load the process into its own memory space.
    /// The process will have been already loaded in stage 1.  This simply assigns
    /// memory maps as necessary.
    pub fn load(&self, allocator: &mut BootConfig, load_offset: usize, pid: XousPid) -> usize {
        println!("Mapping PID {} starting at offset {:08x}", pid, load_offset);
        let mut allocated_bytes = 0;

        let mut current_page_addr: usize = 0;
        let mut previous_addr: usize = 0;

        // The load offset is the end of this process.  Shift it down by one page
        // so we get the start of the first page.
        let mut top = load_offset - PAGE_SIZE;
        let stack_addr = USER_STACK_TOP - 16;

        // Allocate a page to handle the top-level memory translation
        let satp_address = allocator.alloc() as usize;
        allocator.change_owner(pid as XousPid, satp_address);

        // Turn the satp address into a pointer
        println!("    Pagetable @ {:08x}", satp_address);
        let satp = unsafe { &mut *(satp_address as *mut PageTable) };
        allocator.map_page(satp, satp_address, PAGE_TABLE_ROOT_OFFSET, FLG_R | FLG_W | FLG_VALID);

        // Allocate thread 1 for this process
        let thread_address = allocator.alloc() as usize;
        println!("    Thread 1 @ {:08x}", thread_address);
        allocator.map_page(satp, thread_address, CONTEXT_OFFSET, FLG_R | FLG_W | FLG_VALID);
        allocator.change_owner(pid as XousPid, thread_address as usize);

        // Allocate stack pages.
        println!("    Stack");
        for i in 0..STACK_PAGE_COUNT {
            if i == 0 {
                // For the initial stack frame, allocate a valid page
                let sp_page = allocator.alloc() as usize;
                allocator.map_page(
                    satp,
                    sp_page,
                    (stack_addr - PAGE_SIZE * i) & !(PAGE_SIZE - 1),
                    FLG_U | FLG_R | FLG_W | FLG_VALID,
                );
                allocator.change_owner(pid as XousPid, sp_page);
            } else {
                // Reserve every other stack page other than the 1st page.
                allocator.map_page(
                    satp,
                    0,
                    (stack_addr - PAGE_SIZE * i) & !(PAGE_SIZE - 1),
                    FLG_U | FLG_R | FLG_W,
                );
            }
        }

        // Example: Page starts at 0xf0c0 and is 8192 bytes long.
        // 1. Copy 3094 bytes to page 1
        // 2. Copy 4096 bytes to page 2
        // 3. Copy 192 bytes to page 3
        //
        // Example: Page starts at 0xf000 and is 4096 bytes long
        // 1. Copy 4096 bytes to page 1
        //
        // Example: Page starts at 0xf000 and is 128 bytes long
        // 1. Copy 128 bytes to page 1
        //
        // Example: Page starts at 0xf0c0 and is 128 bytes long
        // 1. Copy 128 bytes to page 1
        for section in self.sections {
            if VDBG {println!("    Section @ {:08x}", section.virt as usize);}
            let flag_defaults = FLG_U
                | FLG_R
                | FLG_X
                | FLG_VALID
                | if section.flags() & 1 == 1 { FLG_W } else { 0 }
                | if section.flags() & 4 == 4 { FLG_X } else { 0 };

            if (section.virt as usize) < previous_addr {
                panic!("init section addresses are not strictly increasing");
            }

            let mut this_page = section.virt as usize & !(PAGE_SIZE - 1);
            let mut bytes_to_copy = section.len();

            // If this is not a new page, ensure the uninitialized values from between
            // this section and the previous one are all zeroed out.
            if this_page != current_page_addr || previous_addr == current_page_addr {
                if VDBG {println!("1       {:08x} -> {:08x}", top as usize, this_page);}
                allocator.map_page(satp, top as usize, this_page, flag_defaults);
                allocator.change_owner(pid as XousPid, top as usize);
                allocated_bytes += PAGE_SIZE;
                top -= PAGE_SIZE;
                this_page += PAGE_SIZE;

                // Part 1: Copy the first chunk over.
                let mut first_chunk_size = PAGE_SIZE - (section.virt as usize & (PAGE_SIZE - 1));
                if first_chunk_size > section.len() {
                    first_chunk_size = section.len();
                }
                bytes_to_copy -= first_chunk_size;
            } else {
                if VDBG {println!(
                    "This page is {:08x}, and last page was {:08x}",
                    this_page, current_page_addr
                );}
                // This is a continuation of the previous section, and as a result
                // the memory will have been copied already. Avoid copying this data
                // to a new page.
                let first_chunk_size = PAGE_SIZE - (section.virt as usize & (PAGE_SIZE - 1));
                if VDBG {println!("First chunk size: {}", first_chunk_size);}
                if bytes_to_copy < first_chunk_size {
                    bytes_to_copy = 0;
                    if VDBG {println!("Clamping to 0 bytes");}
                } else {
                    bytes_to_copy -= first_chunk_size;
                    if VDBG {println!(
                        "Clamping to {} bytes by cutting off {} bytes",
                        bytes_to_copy, first_chunk_size
                    );}
                }
                this_page += PAGE_SIZE;
            }

            // Part 2: Copy any full pages.
            while bytes_to_copy >= PAGE_SIZE {
                if VDBG {println!("2       {:08x} -> {:08x}", top as usize, this_page);}
                allocator.map_page(satp, top as usize, this_page, flag_defaults);
                allocator.change_owner(pid as XousPid, top as usize);
                allocated_bytes += PAGE_SIZE;
                top -= PAGE_SIZE;
                this_page += PAGE_SIZE;
                bytes_to_copy -= PAGE_SIZE;
            }

            // Part 3: Copy the final residual partial page
            if bytes_to_copy > 0 {
                let this_page = (section.virt as usize + section.len()) & !(PAGE_SIZE - 1);
                if VDBG {println!("3       {:08x} -> {:08x}", top as usize, this_page);}
                allocator.map_page(satp, top as usize, this_page, flag_defaults);
                allocator.change_owner(pid as XousPid, top as usize);
                allocated_bytes += PAGE_SIZE;
                top -= PAGE_SIZE;
            }

            previous_addr = section.virt as usize + section.len();
            current_page_addr = previous_addr & !(PAGE_SIZE - 1);
        }

        let mut process = &mut allocator.processes[pid as usize - 1];
        process.entrypoint = self.entry_point as usize;
        process.sp = stack_addr;
        process.satp = 0x8000_0000 | ((pid as usize) << 22) | (satp_address >> 12);

        allocated_bytes
    }
}