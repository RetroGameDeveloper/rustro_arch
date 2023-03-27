extern crate libloading;
extern crate libc;

use minifb::{Key, Window, WindowOptions};
use std::time::{Duration, Instant};
use libloading::{Library, Symbol};

const WIDTH: usize = 640;
const HEIGHT: usize = 480;

fn load_core() {
    unsafe {
        let core = Library::new("gambatte_libretro.dylib").expect("Failed to load Core");
        let retro_init: unsafe extern "C" fn() = *(core.get(b"retro_init").unwrap());
        retro_init();
    }
}

fn main() {
    let mut buffer: Vec<u32> = vec![0; WIDTH * HEIGHT];
    let mut window = Window::new(
        "Rust Game",
        WIDTH,
        HEIGHT,
        WindowOptions::default(),
    ).unwrap_or_else(|e| {
        panic!("{}", e);
    });
    // window.limit_update_rate(Some(std::time::Duration::from_micros(16600))); // ~60fps
    
    load_core();
    let mut x: usize = 0;
    let mut y: usize = 0;

    let mut fps_timer = Instant::now();
    let mut fps_counter = 0;

    while window.is_open() && !window.is_key_down(Key::Escape) {
        // Clear the previous pixel to black
        buffer[y * WIDTH + x] = 0x00000000;

        // Calculate fps
        fps_counter += 1;
        let elapsed = fps_timer.elapsed();
        if elapsed >= Duration::from_secs(1) {
            let fps = fps_counter as f64 / elapsed.as_secs_f64();
            window.set_title(&format!("Rust Game (FPS: {:.2})", fps));
            fps_counter = 0;
            fps_timer = Instant::now();
        }

        // Move the pixel when the arrow keys are pressed
        if window.is_key_down(Key::Left) && x > 0 {
            x -= 1;
        }
        if window.is_key_down(Key::Right) && x < WIDTH - 1 {
            x += 1;
        }
        if window.is_key_down(Key::Up) && y > 0 {
            y -= 1;
        }
        if window.is_key_down(Key::Down) && y < HEIGHT - 1 {
            y += 1;
        }

        // Set the current pixel to blue
        buffer[y * WIDTH + x] = 0x0000FFFF;

        // Update the window buffer and display the changes
        window.update_with_buffer(&buffer, WIDTH, HEIGHT).unwrap();
    }
}
