use crate::xip::*;
use crate::xip::minielf::MiniElf;
use crate::xip::bootconfig::BootConfig;

/// Stage 2 bootloader
/// This sets up the MMU and loads both PID1 and the kernel into RAM.
pub fn phase_2(cfg: &mut BootConfig) {
    let args = cfg.args;

    // This is the offset in RAM where programs are loaded from.
    let mut process_offset = cfg.sram_start as usize + cfg.sram_size - cfg.init_size;
    println!("Procesess start out @ {:08x}", process_offset);

    // Go through all Init processes and the kernel, setting up their
    // page tables and mapping memory to them.
    let mut pid = 2;
    for tag in args.iter() {
        if tag.name == u32::from_le_bytes(*b"IniE") {
            let inie = MiniElf::new(&tag);
            println!("Mapping program into memory");
            process_offset -= inie.load(cfg, process_offset, pid);
            pid += 1;
        } else if tag.name == u32::from_le_bytes(*b"XKrn") {
            println!("Mapping kernel into memory");
            let xkrn = unsafe { &*(tag.data.as_ptr() as *const ProgramDescription) };
            let load_size_rounded = ((xkrn.text_size as usize + PAGE_SIZE - 1) & !(PAGE_SIZE - 1))
                + (((xkrn.data_size + xkrn.bss_size) as usize + PAGE_SIZE - 1) & !(PAGE_SIZE - 1));
            xkrn.load(cfg, process_offset - load_size_rounded, 1);
            process_offset -= load_size_rounded;
        }
    }

    println!("Done loading.");
    let krn_struct_start = cfg.sram_start as usize + cfg.sram_size - cfg.init_size;
    let krn_l1_pt_addr = cfg.processes[0].satp << 12;
    assert!(krn_struct_start & (PAGE_SIZE - 1) == 0);
    let krn_pg1023_ptr = unsafe { (krn_l1_pt_addr as *const usize).add(1023).read() };

    // Map boot-generated kernel structures into the kernel
    let satp = unsafe { &mut *(krn_l1_pt_addr as *mut PageTable) };
    for addr in (0..cfg.init_size-GUARD_MEMORY_BYTES).step_by(PAGE_SIZE as usize) {
        cfg.map_page(
            satp,
            addr + krn_struct_start,
            addr + KERNEL_ARGUMENT_OFFSET,
            FLG_R | FLG_W | FLG_VALID,
        );
        cfg.change_owner(1 as XousPid, (addr + krn_struct_start) as usize);
    }

    // Copy the kernel's "MMU Page 1023" into every process.
    // This ensures a context switch into the kernel can
    // always be made, and that the `stvec` is always valid.
    // Since it's a megapage, all we need to do is write
    // the one address to get all 4MB mapped.
    println!("Mapping MMU page 1023 to all processes");
    for process in cfg.processes[1..].iter() {
        let l1_pt_addr = process.satp << 12;
        unsafe { (l1_pt_addr as *mut usize).add(1023).write(krn_pg1023_ptr) };
    }

    if VDBG {
        println!("PID1 pagetables:");
        print_pagetable(cfg.processes[0].satp);
        println!();
        println!();
        for (_pid, process) in cfg.processes[1..].iter().enumerate() {
            println!("PID{} pagetables:", _pid + 2);
            print_pagetable(process.satp);
            println!();
            println!();
        }
    }
    println!(
        "Runtime Page Tracker: {} bytes",
        cfg.runtime_page_tracker.len()
    );
    // mark pages used by suspend/resume according to their needs
    cfg.runtime_page_tracker[cfg.sram_size / PAGE_SIZE - 1] = 1; // claim the loader stack -- do not allow tampering, as it contains backup kernel args
    cfg.runtime_page_tracker[cfg.sram_size / PAGE_SIZE - 2] = 1; // 8k in total (to allow for digital signatures to be computed)
    cfg.runtime_page_tracker[cfg.sram_size / PAGE_SIZE - 3] = 0; // allow clean suspend page to be mapped in Xous
}