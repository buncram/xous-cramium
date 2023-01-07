use crate::println;

pub const SIGBLOCK_SIZE: usize = 0x1000;

const VERSION_STR: &'static str = "Xous(Cramium) OS Loader v0.1.0\n\r";
// v0.1.0 -- initial version

pub const STACK_LEN: u32 = 8192 - (7 * 4); // 7 words for backup kernel args
pub const STACK_TOP: u32 = 0x4100_0000 - STACK_LEN;

use utralib::generated::*;

struct Point {
    x: i16,
    y: i16,
}
#[derive(PartialEq, Eq)]
enum Color {
    Light,
    Dark
}
const FB_WIDTH_WORDS: usize = 11;
const FB_WIDTH_PIXELS: usize = 336;
const FB_LINES: usize = 536;
const FB_SIZE: usize = FB_WIDTH_WORDS * FB_LINES; // 44 bytes by 536 lines
// this font is from the embedded graphics crate https://docs.rs/embedded-graphics/0.7.1/embedded_graphics/
const FONT_IMAGE: &'static [u8] = include_bytes!("font6x12_1bpp.raw");
const CHAR_HEIGHT: u32 = 12;
const CHAR_WIDTH: u32 = 6;
const FONT_IMAGE_WIDTH: u32 = 96;
const LEFT_MARGIN: i16 = 10;

struct Gfx {
    csr: utralib::CSR<u32>,
    fb: &'static mut [u32],
}
impl<'a> Gfx {
    pub fn init(&mut self, clk_mhz: u32) {
        self.csr.wfo(utra::memlcd::PRESCALER_PRESCALER, (clk_mhz / 2_000_000) - 1);
    }
    #[allow(dead_code)]
    pub fn update_all(&mut self) {
        self.csr.wfo(utra::memlcd::COMMAND_UPDATEALL, 1);
    }
    pub fn update_dirty(&mut self) {
        self.csr.wfo(utra::memlcd::COMMAND_UPDATEDIRTY, 1);
    }
    pub fn busy(&self) -> bool {
        if self.csr.rf(utra::memlcd::BUSY_BUSY) == 1 {
            true
        } else {
            false
        }
    }
    pub fn flush(&mut self) {
        self.update_dirty();
        while self.busy() {}
        // clear the dirty bits
        for lines in 0..FB_LINES {
            self.fb[lines * FB_WIDTH_WORDS + (FB_WIDTH_WORDS - 1)] &= 0x0000_FFFF;
        }
    }

    fn char_offset(&self, c: char) -> u32 {
        let fallback = ' ' as u32 - ' ' as u32;
        if c < ' ' {
            return fallback;
        }
        if c <= '~' {
            return c as u32 - ' ' as u32;
        }
        fallback
    }
    fn put_digit(&mut self, d: u8, pos: &mut Point) {
        let mut buf: [u8; 4] = [0; 4]; // stack buffer for the character encoding
        let nyb = d & 0xF;
        if nyb < 10 {
            self.msg(((nyb + 0x30) as char).encode_utf8(&mut buf), pos);
        } else {
            self.msg(((nyb + 0x61 - 10) as char).encode_utf8(&mut buf), pos);
        }
    }
    fn put_hex(&mut self, c: u8, pos: &mut Point) {
        self.put_digit(c >> 4, pos);
        self.put_digit(c & 0xF, pos);
    }
    pub fn hex_word(&mut self, word: u32, pos: &mut Point) {
        for &byte in word.to_be_bytes().iter() {
            self.put_hex(byte, pos);
        }
    }
    pub fn msg(&mut self, text: &'a str, pos: &mut Point) {
        // this routine is adapted from the embedded graphics crate https://docs.rs/embedded-graphics/0.7.1/embedded_graphics/
        let char_per_row = FONT_IMAGE_WIDTH / CHAR_WIDTH;
        let mut idx = 0;
        let mut x_update: i16 = 0;
        for current_char in text.chars() {
            let mut char_walk_x = 0;
            let mut char_walk_y = 0;

            loop {
                // Char _code_ offset from first char, most often a space
                // E.g. first char = ' ' (32), target char = '!' (33), offset = 33 - 32 = 1
                let char_offset = self.char_offset(current_char);
                let row = char_offset / char_per_row;

                // Top left corner of character, in pixels
                let char_x = (char_offset - (row * char_per_row)) * CHAR_WIDTH;
                let char_y = row * CHAR_HEIGHT;

                // Bit index
                // = X pixel offset for char
                // + Character row offset (row 0 = 0, row 1 = (192 * 8) = 1536)
                // + X offset for the pixel block that comprises this char
                // + Y offset for pixel block
                let bitmap_bit_index = char_x
                    + (FONT_IMAGE_WIDTH * char_y)
                    + char_walk_x
                    + (char_walk_y * FONT_IMAGE_WIDTH);

                let bitmap_byte = bitmap_bit_index / 8;
                let bitmap_bit = 7 - (bitmap_bit_index % 8);

                let color = if FONT_IMAGE[bitmap_byte as usize] & (1 << bitmap_bit) != 0 {
                    Color::Light
                } else {
                    Color::Dark
                };

                let x = pos.x
                    + (CHAR_WIDTH * idx as u32) as i16
                    + char_walk_x as i16;
                let y = pos.y + char_walk_y as i16;

                // draw color at x, y
                if (current_char as u8 != 0xd) && (current_char as u8 != 0xa) { // don't draw CRLF specials
                    self.draw_pixel(Point{x, y}, color);
                }

                char_walk_x += 1;

                if char_walk_x >= CHAR_WIDTH {
                    char_walk_x = 0;
                    char_walk_y += 1;

                    // Done with this char, move on to the next one
                    if char_walk_y >= CHAR_HEIGHT {
                        if current_char as u8 == 0xd { // '\n'
                            pos.y += CHAR_HEIGHT as i16;
                        } else if current_char as u8 == 0xa { // '\r'
                            pos.x = LEFT_MARGIN as i16;
                            x_update = 0;
                        } else {
                            idx += 1;
                            x_update += CHAR_WIDTH as i16;
                        }

                        break;
                    }
                }
            }
        }
        pos.x += x_update;
        self.flush();
    }
    pub fn draw_pixel(&mut self, pix: Point, color: Color) {
        let mut clip_y: usize = pix.y as usize;
        if clip_y >= FB_LINES {
            clip_y = FB_LINES - 1;
        }
        let clip_x: usize = pix.x as usize;
        if clip_x >= FB_WIDTH_PIXELS {
            clip_y = FB_WIDTH_PIXELS - 1;
        }
        if color == Color::Light {
            self.fb[(clip_x + clip_y * FB_WIDTH_WORDS * 32) / 32] |= 1 << (clip_x % 32)
        } else {
            self.fb[(clip_x + clip_y * FB_WIDTH_WORDS * 32) / 32] &= !(1 << (clip_x % 32))
        }
        // set the dirty bit on the line that contains the pixel
        self.fb[clip_y * FB_WIDTH_WORDS + (FB_WIDTH_WORDS - 1)] |= 0x1_0000;
    }
}

// returns true if the kernel is valid
// side-effects the "devboot" register in the gfx engine if devkeys were detected
pub fn validate_xous_img(xous_img_offset: *const u32) -> bool {
    let mut cursor = Point {x: LEFT_MARGIN, y: (FB_LINES as i16 / 2) + 10}; // draw on bottom half
    // clear screen to all black
    let mut gfx = Gfx {
        csr: CSR::new(utra::memlcd::HW_MEMLCD_BASE as *mut u32),
        fb: unsafe{core::slice::from_raw_parts_mut(utralib::HW_MEMLCD_MEM as *mut u32, FB_SIZE)},
    };
    gfx.init(100_000_000);

    // insert a pattern of alternating 0101/1010 to create a "gray effect" on the bottom half of the fb
    // note that the gray has "stripes" every 32 bits but it's visually easier to look at than stripes every other bit
    let (_top, bottom) = gfx.fb.split_at_mut(gfx.fb.len() / 2);
    for (i, word) in bottom.iter_mut().enumerate() {
        if i % 2 == 0 {
            *word = 0xAAAA_AAAA;
        } else {
            *word = 0x5555_5555;
        }
    }
    gfx.flush();

    // now characters should actually be able to print
    gfx.msg(VERSION_STR, &mut cursor);
    println!("{}", VERSION_STR);

    // check the stack usage
    let stack: &[u32] = unsafe{core::slice::from_raw_parts(STACK_TOP as *const u32, (STACK_LEN as usize / core::mem::size_of::<u32>()) as usize)};
    let mut unused_stack_words = 0;
    for &word in stack.iter() {
        if word != 0xACE0BACE {
            break;
        }
        unused_stack_words += 1;
    }
    println!("Free stack after signature check: {} bytes", unused_stack_words * 4);
    gfx.msg("Free stack after sigcheck: 0x", &mut cursor);
    gfx.hex_word(unused_stack_words * 4, &mut cursor);

    gfx.msg("\n\r\n\rLoading kernel...\n\r", &mut cursor);
    println!("Everything checks out, loading to kernel...");
    true
}
