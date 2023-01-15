use utralib::generated::*;
fn main() {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);

    let csr = xous::syscall::map_memory(
        xous::MemoryAddress::new(utra::main::HW_MAIN_BASE),
        None,
        4096,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    )
    .expect("couldn't map Core Control CSR range");
    let mut core_csr = CSR::new(csr.as_mut_ptr() as *mut u32);

    core_csr.wfo(utra::main::REPORT_REPORT, 0x600d_0000);
    log::info!("my PID is {}", xous::process::id());
    let tt = xous_api_ticktimer::Ticktimer::new().unwrap();
    let mut total = 0;
    let mut iter = 0;
    loop {
        // this conjures a scalar message
        core_csr.wfo(utra::main::REPORT_REPORT, 0x1111_0000 + iter);
        let now = tt.elapsed_ms();
        core_csr.wfo(utra::main::REPORT_REPORT, 0x2222_0000 + iter);
        total += now;
        if now > 0x20 && now < 0x30 {
            tt.sleep_ms(1).unwrap();
        } else if now < 0x40 {
            tt.sleep_ms(1).ok();
        } else if now > 0x50 {
            break;
        }
        // something lame to just conjure a memory message
        core_csr.wfo(utra::main::REPORT_REPORT, 0x3333_0000 + iter);
        let version = tt.get_version();
        core_csr.wfo(utra::main::REPORT_REPORT, 0x4444_0000 + iter);
        total += version.len() as u64;
        iter += 1;
        core_csr.wfo(utra::main::REPORT_REPORT, total as u32);
    }
    core_csr.wfo(utra::main::REPORT_REPORT, 0x600d_c0de);
    println!("Elapsed: {}", total);
    core_csr.wfo(utra::main::REPORT_REPORT, 0x6969_6969);
}
