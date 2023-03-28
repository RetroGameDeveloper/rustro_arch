extern crate libloading;
extern crate libc;
use clap::{App, Arg};

use libretro_sys::{CoreAPI, GameInfo};
use minifb::{Key, Window, WindowOptions};
use std::time::{Duration, Instant};
use libloading::{Library};
use std::ffi::{c_void, CString};
use std::{ptr, fs};

const WIDTH: usize = 640;
const HEIGHT: usize = 480;
const EXPECTED_LIB_RETRO_VERSION: u32 = 1;

pub type EnvironmentCallback = unsafe extern "C" fn(command: libc::c_uint, data: *mut libc::c_void) -> bool;

unsafe extern "C" fn libretro_environment_callback(command: u32, data: *mut c_void) -> bool {
    println!("libretro_environment_callback Called with command: {}", command);
    false
}

fn load_core(library_path: String) -> (CoreAPI) {
    unsafe {
        let dylib = Box::leak(Box::new(Library::new(library_path).expect("Failed to load Core")));
        
        let core_api = CoreAPI {
            retro_set_environment: *(dylib.get(b"retro_set_environment").unwrap()),
            retro_set_video_refresh: *(dylib.get(b"retro_set_video_refresh").unwrap()),
            retro_set_audio_sample: *(dylib.get(b"retro_set_audio_sample").unwrap()),
            retro_set_audio_sample_batch: *(dylib.get(b"retro_set_audio_sample_batch").unwrap()),
            retro_set_input_poll: *(dylib.get(b"retro_set_input_poll").unwrap()),
            retro_set_input_state: *(dylib.get(b"retro_set_input_state").unwrap()),

            retro_init: *(dylib.get(b"retro_init").unwrap()),
            retro_deinit: *(dylib.get(b"retro_deinit").unwrap()),

            retro_api_version: *(dylib.get(b"retro_api_version").unwrap()),

            retro_get_system_info: *(dylib.get(b"retro_get_system_info").unwrap()),
            retro_get_system_av_info: *(dylib.get(b"retro_get_system_av_info").unwrap()),
            retro_set_controller_port_device: *(dylib.get(b"retro_set_controller_port_device").unwrap()),

            retro_reset: *(dylib.get(b"retro_reset").unwrap()),
            retro_run: *(dylib.get(b"retro_run").unwrap()),

            retro_serialize_size: *(dylib.get(b"retro_serialize_size").unwrap()),
            retro_serialize: *(dylib.get(b"retro_serialize").unwrap()),
            retro_unserialize: *(dylib.get(b"retro_unserialize").unwrap()),

            retro_cheat_reset: *(dylib.get(b"retro_cheat_reset").unwrap()),
            retro_cheat_set: *(dylib.get(b"retro_cheat_set").unwrap()),

            retro_load_game: *(dylib.get(b"retro_load_game").unwrap()),
            retro_load_game_special: *(dylib.get(b"retro_load_game_special").unwrap()),
            retro_unload_game: *(dylib.get(b"retro_unload_game").unwrap()),

            retro_get_region: *(dylib.get(b"retro_get_region").unwrap()),
            retro_get_memory_data: *(dylib.get(b"retro_get_memory_data").unwrap()),
            retro_get_memory_size: *(dylib.get(b"retro_get_memory_size").unwrap()),
        };

        let api_version = (core_api.retro_api_version)();
        println!("API Version: {}", api_version);
        if (api_version != EXPECTED_LIB_RETRO_VERSION) {
            panic!("The Core has been compiled with a LibRetro API that is unexpected, we expected version to be: {} but it was: {}", EXPECTED_LIB_RETRO_VERSION, api_version)
        }
        (core_api.retro_set_environment)(libretro_environment_callback);
        (core_api.retro_init)();
        return core_api;
    }
}

struct EmulatorState {
    rom_name: String,
    core_name: String,
}

fn parse_command_line_arguments() -> EmulatorState {
    let matches = App::new("RustroArch")
        .arg(
            Arg::with_name("rom_name")
                .help("Sets the path to the ROM file to load")
                .required(true)
                .index(1),
        )
        .arg(
            Arg::with_name("library_name")
                .help("Sets the path to the libRetro core to use")
                .short("L")
                .takes_value(true),
        )
        .get_matches();

    let rom_name = matches.value_of("rom_name").unwrap();
    let library_name = matches.value_of("library_name").unwrap_or("default_library");
    println!("ROM name: {}", rom_name);
    println!("Core Library name: {}", library_name);
    return EmulatorState {
        rom_name: rom_name.to_string(), core_name: library_name.to_string()
    }
    
}

unsafe fn load_rom_file(core_api: CoreAPI, rom_name: String) {
    let rom_name_cptr = CString::new(rom_name.clone()).expect("Failed to create CString").as_ptr();
    let contents = fs::read(rom_name).expect("Failed to read file");
    let data: *const c_void = contents.as_ptr() as *const c_void;
    let game_info = GameInfo {
        path: rom_name_cptr,
        data,
        size: contents.len(),
        meta: ptr::null(),
    };
    (core_api.retro_load_game)(&game_info);
}

fn main() {
    let mut buffer: Vec<u32> = vec![0; WIDTH * HEIGHT];
    let emulator_state = parse_command_line_arguments();
    let mut window = Window::new(
        "RustroArch",
        WIDTH,
        HEIGHT,
        WindowOptions::default(),
    ).unwrap_or_else(|e| {
        panic!("{}", e);
    });
    // window.limit_update_rate(Some(std::time::Duration::from_micros(16600))); // ~60fps
    
    unsafe {
        let core_api = load_core(emulator_state.core_name);
        (core_api.retro_init)();
        println!("About to load ROM: {}", emulator_state.rom_name);
        load_rom_file(core_api, emulator_state.rom_name);
    }

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
            window.set_title(&format!("RustroArch (FPS: {:.2})", fps));
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
