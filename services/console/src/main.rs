use utralib::generated::*;
fn main() {
    let mut report = CSR::new(utra::main::HW_MAIN_BASE as *mut u32);
    report.wfo(utra::main::REPORT_REPORT, 0x600d_0000);
    let tt = xous_api_ticktimer::Ticktimer::new().unwrap();
    let mut total = 0;
    let mut iter = 0;
    loop {
        // this conjures a scalar message
        report.wfo(utra::main::REPORT_REPORT, 0x1111_0000 + iter);
        let now = tt.elapsed_ms();
        report.wfo(utra::main::REPORT_REPORT, 0x2222_0000 + iter);
        total += now;
        if now > 420 {
            break;
        }
        // something lame to just conjure a memory message
        report.wfo(utra::main::REPORT_REPORT, 0x3333_0000 + iter);
        let version = tt.get_version();
        report.wfo(utra::main::REPORT_REPORT, 0x4444_0000 + iter);
        total += version.len() as u64;
        iter += 1;
        report.wfo(utra::main::REPORT_REPORT, total as u32);
    }
    report.wfo(utra::main::REPORT_REPORT, 0x600d_c0de);
    println!("Elapsed: {}", total);
    report.wfo(utra::main::REPORT_REPORT, 0x6969_6969);
}
