extern crate libc;
extern crate libloading;
use clap::{App, Arg};

use libloading::Library;
use libretro_sys::{CoreAPI, GameInfo, PixelFormat, SystemAvInfo, GameGeometry, SystemTiming, LogCallback, LogLevel};
use minifb::{Key, KeyRepeat, Window, WindowOptions};
use std::collections::HashMap;
use std::ffi::{c_void, CString, CStr};
use std::fs::File;
use std::io::Read;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use std::{env, fs, ptr, mem}; // Add this line to import the Read trait
use rodio::{Sink, OutputStream, OutputStreamHandle};
use rodio::buffer::SamplesBuffer;
use std::sync::mpsc::{channel, Sender};
use std::thread;

use gilrs::{Gilrs, Button, Event};


const EXPECTED_LIB_RETRO_VERSION: u32 = 1;

const audio_enable: bool = false;

struct EmulatorState {
    rom_name: String,
    core_name: String,
    frame_buffer: Option<Vec<u32>>,
    audio_data: Option<Vec<i16>>,
    pixel_format: PixelFormat,
    bytes_per_pixel: u8, // its only either 2 or 4 bytes per pixel in libretro
    screen_pitch: u32,
    screen_width: u32,
    screen_height: u32,
    buttons_pressed: Option<Vec<i16>>,
    current_save_slot: u8,
    av_info: Option<SystemAvInfo>,
    game_info: Option<GameInfo>,
    game_info_ext: Option<GameInfoExt>,
    system_directory: Option<CString>
}

static mut CURRENT_EMULATOR_STATE: EmulatorState = EmulatorState {
    rom_name: String::new(),
    core_name: String::new(),
    frame_buffer: None,
    audio_data: None,
    pixel_format: PixelFormat::ARGB8888,
    bytes_per_pixel: 4,
    screen_pitch: 0,
    screen_width: 0,
    screen_height: 0,
    buttons_pressed: None,
    current_save_slot: 0,
    av_info: None,
    game_info: None,
    game_info_ext: None,
    system_directory: None
};

// retro_game_info_ext wasn't in libretro-sys package so declaring it here
pub struct GameInfoExt {
    pub full_path: *const libc::c_char,
    pub archive_path: *const libc::c_char,
    pub archive_file: *const libc::c_char,
    pub dir: *const libc::c_char,
    pub name: *const libc::c_char,
    pub ext: *const libc::c_char,
    pub meta: *const libc::c_char,
    pub data: *const libc::c_void,
    /* Size of game content memory buffer, in bytes */
    pub size: libc::size_t,
    pub file_in_archive: bool,
    pub persistent_data: bool,
}

////////////////////////
// Utility FUnctions
////////////////////////

// Convert the input String to a CString, but be-careful with memory management when sending this to a core..
fn convert_to_cstring(input: String) -> CString {
    CString::new(input).expect("Failed to convert to CString")
}

// print_c_string simply takes in a Cstring(libc::c_char pointer) and prints it to the console
fn print_c_string(c_string_ptr: *const libc::c_char) {
    unsafe {
        if !c_string_ptr.is_null() {
            let c_str = CStr::from_ptr(c_string_ptr);
            if let Ok(rust_string) = c_str.to_str() {
                println!("{}", rust_string);
            }
        }
    }
}

///////////////////////
// Config Functions
///////////////////////
fn get_retroarch_config_path() -> PathBuf {
    return match std::env::consts::OS {
        "windows" => PathBuf::from(env::var("APPDATA").ok().unwrap()).join("retroarch"),
        "macos" => PathBuf::from(env::var("HOME").ok().unwrap())
            .join("Library/Application Support/RetroArch"),
        _ => PathBuf::from(env::var("XDG_CONFIG_HOME").ok().unwrap()).join("retroarch"),
    };
}

fn parse_retroarch_config(config_file: &Path) -> Result<HashMap<String, String>, String> {
    let file = File::open(config_file).map_err(|e| format!("Failed to open file: {}", e))?;
    let reader = BufReader::new(file);
    let mut config_map = HashMap::new();
    for line in reader.lines() {
        let line = line.map_err(|e| format!("Failed to read line: {}", e))?;
        if let Some((key, value)) = line.split_once("=") {
            config_map.insert(
                key.trim().to_string(),
                value.trim().replace("\"", "").to_string(),
            );
        }
    }
    Ok(config_map)
}

fn convert_pixel_array_from_rgb565_to_xrgb8888(color_array: &[u8]) -> Box<[u32]> {
    println!("convert_pixel_array_from_rgb565_to_xrgb8888");
    let bytes_per_pixel = 2;
    assert_eq!(
        color_array.len() % bytes_per_pixel,
        0,
        "color_array length must be a multiple of 2 (16-bits per pixel)"
    );

    let num_pixels = color_array.len() / bytes_per_pixel;
    let mut result = vec![0u32; num_pixels];

    for i in 0..num_pixels {
        // This Rust code is decoding a 16-bit color value, represented by two bytes of data, into its corresponding red, green, and blue components.
        let first_byte = color_array[bytes_per_pixel * i];
        let second_byte = color_array[(bytes_per_pixel * i) + 1];

        // First extract the red component from the first byte. The first byte contains the most significant 8 bits of the 16-bit color value. The & operator performs a bitwise AND operation on first_byte and 0b1111_1000, which extracts the 5 most significant bits of the byte. The >> operator then shifts the extracted bits to the right by 3 positions, effectively dividing by 8, to get the value of the red component on a scale of 0-31.
        let red = (first_byte & 0b1111_1000) >> 3;
        // Next extract the green component from both bytes. The first part of the expression ((first_byte & 0b0000_0111) << 3) extracts the 3 least significant bits of first_byte and shifts them to the left by 3 positions, effectively multiplying by 8. The second part of the expression ((second_byte & 0b1110_0000) >> 5) extracts the 3 most significant bits of second_byte and shifts them to the right by 5 positions, effectively dividing by 32. The two parts are then added together to get the value of the green component on a scale of 0-63.
        let green = ((first_byte & 0b0000_0111) << 3) + ((second_byte & 0b1110_0000) >> 5);
        // Next extract the blue component from the second byte. The & operator performs a bitwise AND operation on second_byte and 0b0001_1111, which extracts the 5 least significant bits of the byte. This gives the value of the blue component on a scale of 0-31.
        let blue = second_byte & 0b0001_1111;

        // Use high bits for empty low bits as we have more bits available in XRGB8888
        let red = (red << 3) | (red >> 2);
        let green = (green << 2) | (green >> 3);
        let blue = (blue << 3) | (blue >> 2);

        // Finally save the pixel data in the result array as an XRGB8888 value
        result[i] = ((red as u32) << 16) | ((green as u32) << 8) | (blue as u32);
    }

    result.into_boxed_slice()
}

unsafe extern "C" fn libretro_set_video_refresh_callback(
    frame_buffer_data: *const libc::c_void,
    width: libc::c_uint,
    height: libc::c_uint,
    pitch: libc::size_t,
) {
    println!("libretro_set_video_refresh_callback width: {} height: {} pitch: {}", width, height, pitch);
    if (frame_buffer_data == ptr::null()) {
        println!("frame_buffer_data was null");
        return;
    }
    let length_of_frame_buffer =
        ((pitch as u32) * height) * CURRENT_EMULATOR_STATE.bytes_per_pixel as u32;
        println!("length_of_frame_buffer: {}", length_of_frame_buffer);
    let buffer_slice = std::slice::from_raw_parts(
        frame_buffer_data as *const u8,
        length_of_frame_buffer as usize,
    );
    println!("got buffer_slice");
    let result = match CURRENT_EMULATOR_STATE.pixel_format {
        PixelFormat::RGB565 => Vec::from(convert_pixel_array_from_rgb565_to_xrgb8888(buffer_slice)),
        PixelFormat::ARGB8888 => {
            println!("ARGB8888 len:{} w*h*p: {}",  buffer_slice.len(), width * height);
            // std::slice::from_raw_parts(buffer_slice.as_ptr() as *const u32, buffer_slice.len()).to_vec() // original code doesn't work in nestopia
            std::slice::from_raw_parts(buffer_slice.as_ptr() as *const u32, buffer_slice.len()/4).to_vec() // dividing by 4 here seems to fix nestopia for some reason
        },
        _ => panic!("Unknown Pixel Format {:?}", CURRENT_EMULATOR_STATE.pixel_format)
    };
    println!("Middle of libretro_set_video_refresh_callback");

    // Wrap the Vec<u8> in an Option and assign it to the frame_buffer field
    CURRENT_EMULATOR_STATE.frame_buffer = Some(result);
    CURRENT_EMULATOR_STATE.screen_height = height;
    CURRENT_EMULATOR_STATE.screen_width = width;
    CURRENT_EMULATOR_STATE.screen_pitch = pitch as u32;
    println!("End of libretro_set_video_refresh_callback")
}

unsafe extern "C" fn libretro_set_input_poll_callback() {
    println!("libretro_set_input_poll_callback")
}

unsafe extern "C" fn libretro_set_input_state_callback(
    port: libc::c_uint,
    device: libc::c_uint,
    index: libc::c_uint,
    id: libc::c_uint,
) -> i16 {
    // println!("libretro_set_input_state_callback port: {} device: {} index: {} id: {}", port, device, index, id);
    let is_pressed = match &CURRENT_EMULATOR_STATE.buttons_pressed {
        Some(buttons_pressed) => buttons_pressed[id as usize],
        None => 0,
    };

    return is_pressed;
}

unsafe extern "C" fn libretro_set_audio_sample_callback(left: i16, right: i16) {
    println!("libretro_set_audio_sample_callback left channel: {} right: {}", left, right);
}

const AUDIO_CHANNELS: usize = 2; // left and right
unsafe extern "C" fn libretro_set_audio_sample_batch_callback(
    audio_data: *const i16,
    frames: libc::size_t,
) -> libc::size_t {
    let audio_slice = std::slice::from_raw_parts(audio_data, frames * AUDIO_CHANNELS);
    CURRENT_EMULATOR_STATE.audio_data = Some(audio_slice.to_vec());
    return frames;
}

unsafe extern "C" fn libretro_log_print_callback(level: LogLevel, fmt: *const libc::c_char) {
    print!("{:?}: ", level);
    print_c_string(fmt);
}

// NOTE: In the implementation of this function make sure you only send CString's to return_data, otherwise the core will not know when the String ends!
unsafe extern "C" fn libretro_environment_callback(command: u32, return_data: *mut c_void) -> bool {
    println!("libretro_environment_callback command:{}", command);
    return match command {
        libretro_sys::ENVIRONMENT_GET_CAN_DUPE => {
            *(return_data as *mut bool) = true; // Set the return_data to the value true
            println!("Set ENVIRONMENT_GET_CAN_DUPE to true");
            false
        }
        libretro_sys::ENVIRONMENT_SET_PIXEL_FORMAT => {
            let pixel_format = *(return_data as *const u32);
            let pixel_format_as_enum = PixelFormat::from_uint(pixel_format).unwrap();
            CURRENT_EMULATOR_STATE.pixel_format = pixel_format_as_enum;
            match pixel_format_as_enum {
                PixelFormat::ARGB1555 => {
                    println!(
                        "Core will send us pixel data in the RETRO_PIXEL_FORMAT_0RGB1555 format"
                    );
                    CURRENT_EMULATOR_STATE.bytes_per_pixel = 2;
                }
                PixelFormat::RGB565 => {
                    println!(
                        "Core will send us pixel data in the RETRO_PIXEL_FORMAT_RGB565 format"
                    );
                    CURRENT_EMULATOR_STATE.bytes_per_pixel = 2;
                }
                PixelFormat::ARGB8888 => {
                    println!(
                        "Core will send us pixel data in the RETRO_PIXEL_FORMAT_XRGB8888 format"
                    );
                    CURRENT_EMULATOR_STATE.bytes_per_pixel = 4;
                }
                _ => {
                    panic!("Core is trying to use an Unknown Pixel Format")
                }
            }
            true
        }
        libretro_sys::ENVIRONMENT_SET_MEMORY_MAPS => {
            println!("TODO: Handle ENVIRONMENT_SET_MEMORY_MAPS");
            true
        }
        libretro_sys::ENVIRONMENT_SET_CONTROLLER_INFO => {
            println!("TODO: Handle ENVIRONMENT_SET_CONTROLLER_INFO");
            true
        }
        libretro_sys::ENVIRONMENT_GET_VARIABLE_UPDATE => {
            println!("INFO: Ignoring ENVIRONMENT_GET_VARIABLE_UPDATE");
            // Return true when we have changed variables that the core needs to know about, but we don't change anything yet
            false
        }
        // All the GETs not currently supported
        libretro_sys::ENVIRONMENT_GET_CAMERA_INTERFACE => {
            println!("TODO: Handle ENVIRONMENT_GET_CAMERA_INTERFACE");
            true
        }
        libretro_sys::ENVIRONMENT_GET_CORE_ASSETS_DIRECTORY => {
            println!("TODO: Handle ENVIRONMENT_GET_CORE_ASSETS_DIRECTORY");
            true
        }
        libretro_sys::ENVIRONMENT_GET_CURRENT_SOFTWARE_FRAMEBUFFER => {
            println!("TODO: Handle ENVIRONMENT_GET_CURRENT_SOFTWARE_FRAMEBUFFER");
            true
        }
        libretro_sys::ENVIRONMENT_GET_HW_RENDER_INTERFACE => {
            println!("TODO: Handle ENVIRONMENT_GET_HW_RENDER_INTERFACE");
            true
        }
        libretro_sys::ENVIRONMENT_GET_INPUT_DEVICE_CAPABILITIES => {
            println!("TODO: Handle ENVIRONMENT_GET_INPUT_DEVICE_CAPABILITIES");
            true
        }
        libretro_sys::ENVIRONMENT_GET_LANGUAGE => {
            println!("TODO: Handle ENVIRONMENT_GET_LANGUAGE");
            true
        }
        libretro_sys::ENVIRONMENT_GET_LIBRETRO_PATH => {
            println!("TODO: Handle ENVIRONMENT_GET_LIBRETRO_PATH");
            true
        }
        libretro_sys::ENVIRONMENT_GET_LOCATION_INTERFACE => {
            println!("TODO: Handle ENVIRONMENT_GET_LOCATION_INTERFACE");
            true
        }
        libretro_sys::ENVIRONMENT_GET_LOG_INTERFACE => {
            println!("TODO: Handle ENVIRONMENT_GET_LOG_INTERFACE");
            (*(return_data as *mut LogCallback)).log = libretro_log_print_callback;
            true
        }
        libretro_sys::ENVIRONMENT_GET_OVERSCAN => {
            println!("TODO: Handle ENVIRONMENT_GET_OVERSCAN");
            true
        }
        libretro_sys::ENVIRONMENT_GET_PERF_INTERFACE => {
            println!("TODO: Handle ENVIRONMENT_GET_PERF_INTERFACE");
            true
        }
        libretro_sys::ENVIRONMENT_GET_RUMBLE_INTERFACE => {
            println!("TODO: Handle ENVIRONMENT_GET_RUMBLE_INTERFACE");
            true
        }
        libretro_sys::ENVIRONMENT_GET_SAVE_DIRECTORY => {
            println!("TODO: Handle ENVIRONMENT_GET_SAVE_DIRECTORY");
            *(return_data as *mut *const libc::c_char) = CURRENT_EMULATOR_STATE.system_directory.as_ref().unwrap().as_ptr() as *const i8;  // TODO use CString otherwise this will segfault
            true
        }
        libretro_sys::ENVIRONMENT_GET_SENSOR_INTERFACE => {
            println!("TODO: Handle ENVIRONMENT_GET_SENSOR_INTERFACE");
            true
        }
        libretro_sys::ENVIRONMENT_GET_SYSTEM_DIRECTORY => {
            println!("TODO: Handle ENVIRONMENT_GET_SYSTEM_DIRECTORY");
            println!("Rom name: {:?}", CURRENT_EMULATOR_STATE.rom_name);
            println!("Pointer: {:?}", CURRENT_EMULATOR_STATE.rom_name.as_ptr());
           
            *(return_data as *mut *const libc::c_char) = CURRENT_EMULATOR_STATE.system_directory.as_ref().unwrap().as_ptr() as *const i8;
            println!("return_data: {:?}", return_data);
            true
        }
        libretro_sys::ENVIRONMENT_GET_USERNAME => {
            println!("TODO: Handle ENVIRONMENT_GET_USERNAME");
            true
        }
        libretro_sys::ENVIRONMENT_GET_VARIABLE => {
            println!("TODO: Handle ENVIRONMENT_GET_VARIABLE command: {}", command); // 15
            false
        }
        // Rest of the SET_
        libretro_sys::ENVIRONMENT_SET_DISK_CONTROL_INTERFACE=> {
            println!("TODO: Handle ENVIRONMENT_SET_DISK_CONTROL_INTERFACE");
            true
        }
        libretro_sys::ENVIRONMENT_SET_FRAME_TIME_CALLBACK=> {
            println!("TODO: Handle ENVIRONMENT_SET_FRAME_TIME_CALLBACK");
            true
        }
        libretro_sys::ENVIRONMENT_SET_GEOMETRY=> {
            println!("TODO: Handle ENVIRONMENT_SET_GEOMETRY");
            true
        }
        libretro_sys::ENVIRONMENT_SET_HW_RENDER=> {
            println!("TODO: Handle ENVIRONMENT_SET_HW_RENDER");
            true
        }
        libretro_sys::ENVIRONMENT_SET_INPUT_DESCRIPTORS=> {
            println!("TODO: Handle ENVIRONMENT_SET_INPUT_DESCRIPTORS");
            true
        }
        libretro_sys::ENVIRONMENT_SET_KEYBOARD_CALLBACK=> {
            println!("TODO: Handle ENVIRONMENT_SET_KEYBOARD_CALLBACK");
            true
        }
        libretro_sys::ENVIRONMENT_SET_MESSAGE=> {
            println!("TODO: Handle ENVIRONMENT_SET_MESSAGE");
            true
        }
        libretro_sys::ENVIRONMENT_SET_PERFORMANCE_LEVEL=> {
            println!("TODO: Handle ENVIRONMENT_SET_PERFORMANCE_LEVEL");
            true
        }
        libretro_sys::ENVIRONMENT_SET_PROC_ADDRESS_CALLBACK=> {
            println!("TODO: Handle ENVIRONMENT_SET_PROC_ADDRESS_CALLBACK");
            true
        }
        libretro_sys::ENVIRONMENT_SET_ROTATION=> {
            println!("TODO: Handle ENVIRONMENT_SET_ROTATION");
            true
        }
        libretro_sys::ENVIRONMENT_SET_SUBSYSTEM_INFO=> {
            println!("TODO: Handle ENVIRONMENT_SET_SUBSYSTEM_INFO");
            true
        }
        libretro_sys::ENVIRONMENT_SET_SUPPORT_NO_GAME=> {
            println!("TODO: Handle ENVIRONMENT_SET_SUPPORT_NO_GAME");
            true
        }
        libretro_sys::ENVIRONMENT_SET_SYSTEM_AV_INFO=> {
            println!("TODO: Handle ENVIRONMENT_SET_SYSTEM_AV_INFO");
            true
        }
        libretro_sys::ENVIRONMENT_SET_VARIABLES=> {
            println!("TODO: Handle ENVIRONMENT_SET_VARIABLES");
            true
        }
        libretro_sys::ENVIRONMENT_EXPERIMENTAL => {
            println!("TODO: Handle ENVIRONMENT_EXPERIMENTAL");
            true
        }
        libretro_sys::ENVIRONMENT_PRIVATE => {
            println!("TODO: Handle ENVIRONMENT_PRIVATE");
            true
        }
        libretro_sys::ENVIRONMENT_SHUTDOWN => {
            println!("TODO: Handle ENVIRONMENT_SHUTDOWN");
            true
        }
        55 => {
            println!("TODO: Handle RETRO_ENVIRONMENT_SET_CORE_OPTIONS_DISPLAY");
            false
        }
        66 => {
            // TODO: need to return retro_game_info_ext retro_game_info_ext
            println!("TODO: Handle ENVIRONMENT_GET_GAME_INFO_EXT");
            let game_info = CURRENT_EMULATOR_STATE.game_info.clone().unwrap_unchecked();
    
            let data = (return_data as *mut GameInfoExt);
            (*(return_data as *mut GameInfoExt)).full_path = CURRENT_EMULATOR_STATE.rom_name.as_ptr() as *const i8;
            (*(return_data as *mut GameInfoExt)).archive_path = ptr::null();
            (*(return_data as *mut GameInfoExt)).archive_file = ptr::null();
            (*(return_data as *mut GameInfoExt)).ext = CURRENT_EMULATOR_STATE.rom_name.as_ptr() as *const i8;
            (*(return_data as *mut GameInfoExt)).meta = ptr::null();
            (*(return_data as *mut GameInfoExt)).dir = CURRENT_EMULATOR_STATE.rom_name.as_ptr() as *const i8; // TODO: Convert to Cstring
            (*(return_data as *mut GameInfoExt)).name = CURRENT_EMULATOR_STATE.rom_name.as_ptr() as *const i8; // TODO: Convert to Cstring
            (*(return_data as *mut GameInfoExt)).file_in_archive = false;
            (*(return_data as *mut GameInfoExt)).persistent_data = true;
            (*(return_data as *mut GameInfoExt)).size = CURRENT_EMULATOR_STATE.game_info.as_ref().unwrap().size;
            (*(return_data as *mut GameInfoExt)).data = CURRENT_EMULATOR_STATE.game_info.as_ref().unwrap().data;
            println!("Data size {}",  (*(return_data as *mut GameInfoExt)).size);


            true
        }
        _ => {
            println!(
                "libretro_environment_callback Called with command: {}",
                command
            );
            false
        }
    };
}

unsafe fn load_core(library_path: &String) -> (CoreAPI) {
    unsafe {
        let dylib = Box::leak(Box::new(
            Library::new(library_path).expect("Failed to load Core"),
        ));

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
            retro_set_controller_port_device: *(dylib
                .get(b"retro_set_controller_port_device")
                .unwrap()),

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
        (core_api.retro_set_video_refresh)(libretro_set_video_refresh_callback);
        (core_api.retro_set_input_poll)(libretro_set_input_poll_callback);
        (core_api.retro_set_input_state)(libretro_set_input_state_callback);
        (core_api.retro_set_audio_sample)(libretro_set_audio_sample_callback);
        (core_api.retro_set_audio_sample_batch)(libretro_set_audio_sample_batch_callback);
        return core_api;
    }
}

fn setup_config() -> Result<HashMap<String, String>, String> {
    let retro_arch_config_path = get_retroarch_config_path();
    let our_config = parse_retroarch_config(Path::new("./rustroarch.cfg"));
    let retro_arch_config =
        parse_retroarch_config(&retro_arch_config_path.join("config/retroarch.cfg"));
    let mut merged_config: HashMap<String, String> = HashMap::from([
        ("input_player1_a", "a"),
        ("input_player1_b", "s"),
        ("input_player1_x", "z"),
        ("input_player1_y", "x"),
        ("input_player1_l", "q"),
        ("input_player1_r", "w"),
        ("input_player1_down", "down"),
        ("input_player1_up", "up"),
        ("input_player1_left", "left"),
        ("input_player1_right", "right"),
        ("input_player1_select", "space"),
        ("input_player1_start", "enter"),
        ("input_reset", "h"),
        ("input_save_state", "f2"),
        ("input_load_state", "f4"),
        ("input_screenshot", "f8"),
        ("savestate_directory", "./states"),
        ("input_state_slot_decrease", "f6"),
        ("input_state_slot_increase", "f7"),
        // ("audio_enable", "true"),
    ])
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    match retro_arch_config {
        Ok(config) => merged_config.extend(config),
        _ => println!("We don't have RetroArch config"),
    }
    match our_config {
        Ok(config) => merged_config.extend(config),
        _ => println!("We don't have RustroArch config",),
    }
    // println!("retro_arch_config_path: {} merged_config: {:?}", retro_arch_config_path.join("config/retroarch.cfg").display(), merged_config);
    Ok(merged_config.clone())
}

unsafe fn parse_command_line_arguments() {
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
    let library_name = matches
        .value_of("library_name")
        .unwrap_or("default_library");
    println!("ROM name: {}", rom_name);
    println!("Core Library name: {}", library_name);
    CURRENT_EMULATOR_STATE.rom_name = rom_name.to_string();
    CURRENT_EMULATOR_STATE.core_name = library_name.to_string();
}

unsafe fn load_rom_file(core_api: &CoreAPI, rom_name: &String) -> bool {
    println!("Loading ROM file: {:?}", rom_name);
    let rom_name_cptr = CString::new(rom_name.clone())
        .expect("Failed to create CString")
        .as_ptr();
    let contents = fs::read(rom_name).expect("Failed to read file");
    let data: *const c_void = contents.as_ptr() as *const c_void;
    let game_info = GameInfo {
        path: rom_name_cptr,
        data,
        size: contents.len(),
        meta: ptr::null(),
    };
    CURRENT_EMULATOR_STATE.game_info = Some(game_info.clone());

    println!("INFO: Calling retro_load_game in Core");
    let was_load_successful = (core_api.retro_load_game)(&game_info);
    if (!was_load_successful) {
        panic!("Rom Load was not successful");
    }
    println!("ROM was successfully loaded");
    return was_load_successful;
}

unsafe fn send_audio_to_thread(sender: &Sender<&Vec<i16>>) {
    // Send the audio samples to the audio thread using the channel
    match &CURRENT_EMULATOR_STATE.audio_data {
        Some(data) => {
            sender.send(data).unwrap();
        },
        None => {},
    };
    
}

unsafe fn play_audio( sink: &Sink, audio_samples: &Vec<i16>, sample_rate: u32) {
    if !audio_enable {
        return;
    }
    if sink.empty() {
        let audio_slice = std::slice::from_raw_parts(audio_samples.as_ptr() as *const i16, audio_samples.len());
        let source = SamplesBuffer::new(2, sample_rate, audio_slice);
        sink.append(source);
        sink.play();
        sink.sleep_until_end();
    }
}

fn get_save_state_path(
    save_directory: &String,
    game_file_name: &str,
    save_state_index: u8,
) -> Option<PathBuf> {
    // Create a subdirectory named "saves" in the current working directory
    let saves_dir = PathBuf::from(save_directory);
    if !saves_dir.exists() {
        match std::fs::create_dir(&saves_dir) {
            Ok(_) => {}
            Err(err) => panic!(
                "Failed to create save directory: {:?} Error: {}",
                &saves_dir, err
            ),
        }
    }

    // Generate the save state filename
    let game_name = Path::new(game_file_name)
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .replace(" ", "_");
    let save_state_file_name = format!("{}_{}.state", game_name, save_state_index);

    // Combine the saves directory and the save state filename to create the full path
    let save_state_path = saves_dir.join(save_state_file_name);

    Some(save_state_path)
}

unsafe fn save_state(core_api: &CoreAPI, save_directory: &String) {
    let save_state_buffer_size = (core_api.retro_serialize_size)();
    let mut state_buffer: Vec<u8> = vec![0; save_state_buffer_size];
    // Call retro_serialize to create the save state
    (core_api.retro_serialize)(
        state_buffer.as_mut_ptr() as *mut c_void,
        save_state_buffer_size,
    );
    let file_path = get_save_state_path(
        save_directory,
        &CURRENT_EMULATOR_STATE.rom_name,
        CURRENT_EMULATOR_STATE.current_save_slot,
    )
    .unwrap();
    std::fs::write(&file_path, &state_buffer).unwrap();
    println!(
        "Save state saved to: {} with size: {}",
        &file_path.display(),
        save_state_buffer_size
    );
}

unsafe fn load_state(core_api: &CoreAPI, save_directory: &String) {
    let file_path = get_save_state_path(
        save_directory,
        &CURRENT_EMULATOR_STATE.rom_name,
        CURRENT_EMULATOR_STATE.current_save_slot,
    )
    .unwrap();
    let mut state_buffer = Vec::new();
    match File::open(&file_path) {
        Ok(mut file) => {
            // Read the save state file into a buffer
            match file.read_to_end(&mut state_buffer) {
                Ok(_) => {
                    // Call retro_unserialize to apply the save state
                    let result = (core_api.retro_unserialize)(
                        state_buffer.as_mut_ptr() as *mut c_void,
                        state_buffer.len() as usize,
                    );
                    if result {
                        println!("Save state loaded from: {}", &file_path.display());
                    } else {
                        println!("Failed to load save state: error code {}", result);
                    }
                }
                Err(err) => println!("Error reading save state file: {}", err),
            }
        }
        Err(_) => println!("Save state file not found"),
    }
}

fn setup_key_device_map(config: &HashMap<String, String>) -> HashMap<&String, usize> {
    return HashMap::from([
        (
            &config["input_player1_a"],
            libretro_sys::DEVICE_ID_JOYPAD_A as usize,
        ),
        (
            &config["input_player1_b"],
            libretro_sys::DEVICE_ID_JOYPAD_B as usize,
        ),
        (
            &config["input_player1_x"],
            libretro_sys::DEVICE_ID_JOYPAD_X as usize,
        ),
        (
            &config["input_player1_y"],
            libretro_sys::DEVICE_ID_JOYPAD_Y as usize,
        ),
        (
            &config["input_player1_l"],
            libretro_sys::DEVICE_ID_JOYPAD_L as usize,
        ),
        (
            &config["input_player1_r"],
            libretro_sys::DEVICE_ID_JOYPAD_R as usize,
        ),
        (
            &config["input_player1_down"],
            libretro_sys::DEVICE_ID_JOYPAD_DOWN as usize,
        ),
        (
            &config["input_player1_up"],
            libretro_sys::DEVICE_ID_JOYPAD_UP as usize,
        ),
        (
            &config["input_player1_right"],
            libretro_sys::DEVICE_ID_JOYPAD_RIGHT as usize,
        ),
        (
            &config["input_player1_left"],
            libretro_sys::DEVICE_ID_JOYPAD_LEFT as usize,
        ),
        (
            &config["input_player1_start"],
            libretro_sys::DEVICE_ID_JOYPAD_START as usize,
        ),
        (
            &config["input_player1_select"],
            libretro_sys::DEVICE_ID_JOYPAD_SELECT as usize,
        ),
    ]);
}
fn setup_joypad_device_map() -> HashMap<Button, usize> {
    return HashMap::from([
        (
            Button::South,
            libretro_sys::DEVICE_ID_JOYPAD_A as usize,
        ),
        (
            Button::East,
            libretro_sys::DEVICE_ID_JOYPAD_B as usize,
        ),
        (
            Button::West,
            libretro_sys::DEVICE_ID_JOYPAD_X as usize,
        ),
        (
            Button::North,
            libretro_sys::DEVICE_ID_JOYPAD_Y as usize,
        ),
        (
            Button::LeftTrigger,
            libretro_sys::DEVICE_ID_JOYPAD_L as usize,
        ),
        (
            Button::LeftTrigger2,
            libretro_sys::DEVICE_ID_JOYPAD_L2 as usize,
        ),
        (
            Button::RightTrigger,
            libretro_sys::DEVICE_ID_JOYPAD_R as usize,
        ),
        (
            Button::RightTrigger2,
            libretro_sys::DEVICE_ID_JOYPAD_R2 as usize,
        ),
        (
            Button::DPadDown,            
            libretro_sys::DEVICE_ID_JOYPAD_DOWN as usize,
        ),
        (
            Button::DPadUp,
            libretro_sys::DEVICE_ID_JOYPAD_UP as usize,
        ),
        (
            Button::DPadRight,
            libretro_sys::DEVICE_ID_JOYPAD_RIGHT as usize,
        ),
        (
            Button::DPadLeft,
            libretro_sys::DEVICE_ID_JOYPAD_LEFT as usize,
        ),
        (
            Button::Start,
            libretro_sys::DEVICE_ID_JOYPAD_START as usize,
        ),
        (
            Button::Select,
            libretro_sys::DEVICE_ID_JOYPAD_SELECT as usize,
        ),
    ]);
}

fn main() {
    unsafe { parse_command_line_arguments() };
    let config = setup_config().unwrap();

    let key_device_map = setup_key_device_map(&config);
    let joypad_device_map = setup_joypad_device_map();

    println!("Setting up minifb window");
    let mut window =
        Window::new("RustroArch", 640, 480, WindowOptions::default()).unwrap_or_else(|e| {
            panic!("{}", e);
        });

    let mut fps_timer = Instant::now();
    let mut fps_counter = 0;
    let core_api;

    println!("Setting up Audio Thread");
    // Create a channel for passing audio samples from the main thread to the audio thread
    let (sender, receiver) = channel();
    
    // Spawn a new thread to play back audio
    if (audio_enable) {
        let audio_thread = thread::spawn(move || {
            println!("Audio Thread Started");
            let sample_rate = unsafe { match &CURRENT_EMULATOR_STATE.av_info {
                Some(av_info) => av_info.timing.sample_rate,
                None => 0.0
            }
            };
            let (_stream, stream_handle) = OutputStream::try_default().unwrap();
            let sink = Sink::try_new(&stream_handle).unwrap();
            loop {
                // Receive the next set of audio samples from the channel
                let audio_samples = receiver.recv().unwrap();
                unsafe { play_audio(&sink, audio_samples, sample_rate as u32); }
            }
        });
    }

    println!("Gamepad Setup");
    let mut gilrs = Gilrs::new().unwrap();
    let mut active_gamepad = None;

    let mut av_info = SystemAvInfo {
        geometry: GameGeometry {
            base_width: 0,
            base_height: 0,
            max_width: 0,
            max_height: 0,
            aspect_ratio: 0.0,
        },
        timing: SystemTiming {
            fps: 0.0,
            sample_rate: 0.0,
        },
    };
    unsafe {
        println!("Setting up Core");
        core_api = load_core(&CURRENT_EMULATOR_STATE.core_name);
        (core_api.retro_init)();
        (core_api.retro_get_system_av_info)(&mut av_info);
        println!("AV Info: {:?}", &av_info);
        CURRENT_EMULATOR_STATE.av_info = Some(av_info.clone());
        // Environment variables
        CURRENT_EMULATOR_STATE.system_directory = Some(CString::new("System").unwrap());

        println!("About to load ROM: {:?}", CURRENT_EMULATOR_STATE.rom_name);
        load_rom_file(&core_api, &CURRENT_EMULATOR_STATE.rom_name);
    }

    let fps = av_info.timing.fps as u64;
    window.limit_update_rate(Some(std::time::Duration::from_micros(1000000/fps)));
    while window.is_open() && !window.is_key_down(Key::Escape) {
        // Call the libRetro core every frame
        unsafe {
            (core_api.retro_run)();
        }

        // Calculate fps
        fps_counter += 1;
        let elapsed = fps_timer.elapsed();
        if elapsed >= Duration::from_secs(1) {
            let fps = fps_counter as f64 / elapsed.as_secs_f64();
            window.set_title(&format!("RustroArch (FPS: {:.2})", fps));
            fps_counter = 0;
            fps_timer = Instant::now();
        }

        let mut this_frames_pressed_buttons = vec![0; 16];

        let mini_fb_keys = window.get_keys_pressed(KeyRepeat::Yes).unwrap();

        // Gamepad input Handling
        // Examine new events
        while let Some(Event { id, event, time }) = gilrs.next_event() {
            // println!("{:?} New event from {}: {:?}", time, id, event);
            active_gamepad = Some(id);
        }

        // You can also use cached gamepad state
        if let Some(gamepad) = active_gamepad.map(|id| gilrs.gamepad(id)) {
            for button in [Button::South, Button::North, Button::East, Button::West, Button::Start, Button::Select, Button::DPadDown, Button::DPadUp, Button::DPadLeft, Button::DPadRight, Button::LeftTrigger, Button::LeftTrigger2, Button::RightTrigger, Button::RightTrigger2] {
                if gamepad.is_pressed(button) {
                    println!("Button Pressed: {:?}", button);
                    let libretro_button = joypad_device_map.get(&button).unwrap();
                    this_frames_pressed_buttons[*libretro_button] = 1;
                }
            }
        }

        unsafe {
            // Input Handling for the keys pressed in minifb cargo
            for key in mini_fb_keys {
                let key_as_string = format!("{:?}", key).to_ascii_lowercase();

                if let Some(libretro_button_id) = key_device_map.get(&key_as_string) {
                    this_frames_pressed_buttons[*libretro_button_id] = 1;
                    continue;
                }
                if &key_as_string == &config["input_save_state"] {
                    save_state(&core_api, &config["savestate_directory"]);
                    continue;
                }
                if &key_as_string == &config["input_load_state"] {
                    load_state(&core_api, &config["savestate_directory"]);
                    continue;
                }
                if &key_as_string == &config["input_state_slot_increase"] {
                    if CURRENT_EMULATOR_STATE.current_save_slot != 255 {
                        CURRENT_EMULATOR_STATE.current_save_slot += 1;
                        println!(
                            "Current save slot increased to: {}",
                            CURRENT_EMULATOR_STATE.current_save_slot
                        )
                    }
                    continue;
                }
                if &key_as_string == &config["input_state_slot_decrease"] {
                    if CURRENT_EMULATOR_STATE.current_save_slot != 0 {
                        CURRENT_EMULATOR_STATE.current_save_slot -= 1;
                        println!(
                            "Current save slot decreased to: {}",
                            CURRENT_EMULATOR_STATE.current_save_slot
                        )
                    }
                    continue;
                }
                println!("Unhandled Key Pressed: {} ", key_as_string);
            }

            CURRENT_EMULATOR_STATE.buttons_pressed = Some(this_frames_pressed_buttons);
            send_audio_to_thread(&sender);

            match &CURRENT_EMULATOR_STATE.frame_buffer {
                Some(buffer) => {
                    let width = (CURRENT_EMULATOR_STATE.screen_pitch
                        / CURRENT_EMULATOR_STATE.bytes_per_pixel as u32)
                        as usize;
                    let height = CURRENT_EMULATOR_STATE.screen_height as usize;
                    let slice_of_pixel_buffer: &[u32] =
                        std::slice::from_raw_parts(buffer.as_ptr() as *const u32, buffer.len()); // convert to &[u32] slice reference
                    if slice_of_pixel_buffer.len() < width * height * 4 {
                        // The frame buffer isn't big enough so lets add additional pixels just so we can display it
                        let mut vec: Vec<u32> = slice_of_pixel_buffer.to_vec();
                        // println!("Frame Buffer wasn't big enough");
                        vec.resize((width * height * 4) as usize, 0x0000FFFF); // Add any missing pixels with colour blue
                        window.update_with_buffer(&vec, width, height).unwrap();
                    } else {
                        window
                            .update_with_buffer(&slice_of_pixel_buffer, width, height)
                            .unwrap();
                    }
                }
                None => {
                    println!("We don't have a buffer to display");
                }
            }
        }
    }
    // Cleanup at the end
}
