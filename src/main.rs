extern crate libc;
extern crate libloading;
use clap::{App, Arg};
use log::{debug, error, info, trace, warn};

use libloading::Library;
use libretro_sys::{
    CoreAPI, GameGeometry, GameInfo, LogCallback, LogLevel, PixelFormat, SystemAvInfo, SystemTiming,
};
use minifb::{Key, KeyRepeat, Window, WindowOptions};
use rodio::buffer::SamplesBuffer;
use rodio::{OutputStream, Sink};
use std::collections::HashMap;
use std::ffi::{c_void, CStr, CString};
use std::fs::File;
use std::io::Read;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use std::thread;
use std::time::{Duration, Instant};
use std::{env, fs, ptr}; // Add this line to import the Read trait

use gilrs::{Button, Event, Gilrs};

const EXPECTED_LIB_RETRO_VERSION: u32 = 1;

const AUDIO_ENABLE: bool = false;

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
    #[allow(dead_code)] // TODO: Implement full GameInfoExt handling
    game_info_ext: Option<GameInfoExt>,
    system_directory: Option<CString>,
    // New CString fields for ROM path components
    rom_full_path_c: Option<CString>,
    rom_dir_c: Option<CString>,
    rom_name_c: Option<CString>,
    rom_ext_c: Option<CString>,
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
    system_directory: None,
    rom_full_path_c: None,
    rom_dir_c: None,
    rom_name_c: None,
    rom_ext_c: None,
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
#[allow(dead_code)]
fn convert_to_cstring(input: String) -> CString {
    CString::new(input).expect("Failed to convert to CString")
}

// print_c_string simply takes in a Cstring(libc::c_char pointer) and prints it to the console
fn print_c_string(c_string_ptr: *const libc::c_char) {
    unsafe {
        if !c_string_ptr.is_null() {
            let c_str = CStr::from_ptr(c_string_ptr);
            if let Ok(rust_string) = c_str.to_str() {
                info!("{}", rust_string); // Changed from println!
            }
        }
    }
}

///////////////////////
// Config Functions
///////////////////////
fn get_retroarch_config_path_for_os(
    os_name: &str,
    home_dir: Option<&str>,
    appdata_dir: Option<&str>,
    xdg_config_home_dir: Option<&str>,
) -> PathBuf {
    match os_name {
        "windows" => {
            let appdata = appdata_dir
                .expect("APPDATA environment variable not found or is invalid for Windows");
            PathBuf::from(appdata).join("retroarch")
        }
        "macos" => {
            let home = home_dir.expect("HOME environment variable not found or is invalid for macOS");
            PathBuf::from(home).join("Library/Application Support/RetroArch")
        }
        _ => { // Default to Linux/XDG behavior
            let xdg_config_home = xdg_config_home_dir
                .expect("XDG_CONFIG_HOME environment variable not found or is invalid for Linux/other");
            PathBuf::from(xdg_config_home).join("retroarch")
        }
    }
}

pub fn get_retroarch_config_path() -> PathBuf {
    get_retroarch_config_path_for_os(
        std::env::consts::OS,
        std::env::var("HOME").ok().as_deref(),
        std::env::var("APPDATA").ok().as_deref(),
        std::env::var("XDG_CONFIG_HOME").ok().as_deref(),
    )
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
    debug!("convert_pixel_array_from_rgb565_to_xrgb8888"); // Changed from println!
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
    trace!( // Changed from println!
        "libretro_set_video_refresh_callback width: {} height: {} pitch: {}",
        width, height, pitch
    );
    if frame_buffer_data.is_null() {
        warn!("frame_buffer_data was null"); // Changed from println!
        return;
    }
    // The pitch is the number of bytes in a scanline.
    // The total buffer size is pitch * height.
    let length_of_frame_buffer = (pitch as u32) * height; 
    trace!("length_of_frame_buffer: {}", length_of_frame_buffer);
    let buffer_slice = std::slice::from_raw_parts(
        frame_buffer_data as *const u8,
        length_of_frame_buffer as usize, // length_of_frame_buffer is already in bytes
    );
    trace!("got buffer_slice");
    let result = match unsafe { (*(&raw const CURRENT_EMULATOR_STATE)).pixel_format } {
        PixelFormat::RGB565 => Vec::from(convert_pixel_array_from_rgb565_to_xrgb8888(buffer_slice)),
        PixelFormat::ARGB8888 => {
            trace!( 
                "ARGB8888 buffer_slice.len():{}, width*height: {}", // Log actual byte length and expected pixels
                buffer_slice.len(),
                width * height
            );
            // For ARGB8888, each pixel is 4 bytes (u32). Convert byte slice to u32 slice.
            // The length of the u32 slice is the total byte length divided by 4 (size of u32).
            std::slice::from_raw_parts(buffer_slice.as_ptr() as *const u32, buffer_slice.len() / 4)
                .to_vec()
        }
        _ => panic!(
            "Unknown Pixel Format {:?}",
            unsafe { (*(&raw const CURRENT_EMULATOR_STATE)).pixel_format }
        ),
    };
    trace!("Middle of libretro_set_video_refresh_callback"); // Changed from println!

    // Wrap the Vec<u8> in an Option and assign it to the frame_buffer field
    CURRENT_EMULATOR_STATE.frame_buffer = Some(result);
    CURRENT_EMULATOR_STATE.screen_height = height;
    CURRENT_EMULATOR_STATE.screen_width = width;
    CURRENT_EMULATOR_STATE.screen_pitch = pitch as u32;
    trace!("End of libretro_set_video_refresh_callback") // Changed from println!
}

unsafe extern "C" fn libretro_set_input_poll_callback() {
    trace!("libretro_set_input_poll_callback") // Changed from println!
}

unsafe extern "C" fn libretro_set_input_state_callback(
    _port: libc::c_uint,
    _device: libc::c_uint,
    _index: libc::c_uint,
    id: libc::c_uint,
) -> i16 {
    // trace!("libretro_set_input_state_callback port: {} device: {} index: {} id: {}", port, device, index, id);
    

    match unsafe { &(*(&raw const CURRENT_EMULATOR_STATE)).buttons_pressed } {
        Some(buttons_pressed) => buttons_pressed[id as usize],
        None => 0,
    }
}

unsafe extern "C" fn libretro_set_audio_sample_callback(left: i16, right: i16) {
    trace!( // Changed from println!
        "libretro_set_audio_sample_callback left channel: {} right: {}",
        left, right
    );
}

const AUDIO_CHANNELS: usize = 2; // left and right
unsafe extern "C" fn libretro_set_audio_sample_batch_callback(
    audio_data: *const i16,
    frames: libc::size_t,
) -> libc::size_t {
    let audio_slice = std::slice::from_raw_parts(audio_data, frames * AUDIO_CHANNELS);
    CURRENT_EMULATOR_STATE.audio_data = Some(audio_slice.to_vec());
    frames
}

unsafe extern "C" fn libretro_log_print_callback(level: LogLevel, fmt: *const libc::c_char) {
    let message = if !fmt.is_null() {
        CStr::from_ptr(fmt).to_string_lossy().into_owned()
    } else {
        String::from("<null format string>")
    };

    match level {
        LogLevel::Debug => debug!("LibretroCore: {}", message), // Changed from print!
        LogLevel::Info => info!("LibretroCore: {}", message), // Changed from print!
        LogLevel::Warn => warn!("LibretroCore: {}", message), // Changed from print!
        LogLevel::Error => error!("LibretroCore: {}", message), // Changed from print!
        _ => info!("LibretroCore (unknown level {}): {}", level.0, message), // Changed from print!
    }
}

// NOTE: In the implementation of this function make sure you only send CString's to return_data, otherwise the core will not know when the String ends!
unsafe extern "C" fn libretro_environment_callback(command: u32, return_data: *mut c_void) -> bool {
    debug!("libretro_environment_callback command:{}", command); // Changed from println!
    match command {
        libretro_sys::ENVIRONMENT_GET_CAN_DUPE => {
            *(return_data as *mut bool) = true; // Set the return_data to the value true
            info!("Set ENVIRONMENT_GET_CAN_DUPE to true"); // Changed from println!
            false
        }
        libretro_sys::ENVIRONMENT_SET_PIXEL_FORMAT => {
            let pixel_format = *(return_data as *const u32);
            let pixel_format_as_enum = PixelFormat::from_uint(pixel_format).unwrap();
            CURRENT_EMULATOR_STATE.pixel_format = pixel_format_as_enum;
            match pixel_format_as_enum {
                PixelFormat::ARGB1555 => {
                    info!( // Changed from println!
                        "Core will send us pixel data in the RETRO_PIXEL_FORMAT_0RGB1555 format"
                    );
                    CURRENT_EMULATOR_STATE.bytes_per_pixel = 2;
                }
                PixelFormat::RGB565 => {
                    info!( // Changed from println!
                        "Core will send us pixel data in the RETRO_PIXEL_FORMAT_RGB565 format"
                    );
                    CURRENT_EMULATOR_STATE.bytes_per_pixel = 2;
                }
                PixelFormat::ARGB8888 => {
                    info!( // Changed from println!
                        "Core will send us pixel data in the RETRO_PIXEL_FORMAT_XRGB8888 format"
                    );
                    CURRENT_EMULATOR_STATE.bytes_per_pixel = 4;
                }
                _ => {
                    unreachable!("PixelFormat should have been validated by PixelFormat::from_uint earlier or it would have panicked")
                }
            }
            true
        }
        libretro_sys::ENVIRONMENT_SET_MEMORY_MAPS => {
            warn!("TODO: Handle ENVIRONMENT_SET_MEMORY_MAPS"); // Changed from println!
            true
        }
        libretro_sys::ENVIRONMENT_SET_CONTROLLER_INFO => {
            warn!("TODO: Handle ENVIRONMENT_SET_CONTROLLER_INFO"); // Changed from println!
            true
        }
        libretro_sys::ENVIRONMENT_GET_VARIABLE_UPDATE => {
            debug!("INFO: Ignoring ENVIRONMENT_GET_VARIABLE_UPDATE"); // Changed from println!
            // Return true when we have changed variables that the core needs to know about, but we don't change anything yet
            false
        }
        // All the GETs not currently supported
        libretro_sys::ENVIRONMENT_GET_CAMERA_INTERFACE => {
            warn!("TODO: Handle ENVIRONMENT_GET_CAMERA_INTERFACE"); // Changed from println!
            true
        }
        libretro_sys::ENVIRONMENT_GET_CORE_ASSETS_DIRECTORY => {
            warn!("TODO: Handle ENVIRONMENT_GET_CORE_ASSETS_DIRECTORY"); // Changed from println!
            true
        }
        libretro_sys::ENVIRONMENT_GET_CURRENT_SOFTWARE_FRAMEBUFFER => {
            warn!("TODO: Handle ENVIRONMENT_GET_CURRENT_SOFTWARE_FRAMEBUFFER"); // Changed from println!
            true
        }
        libretro_sys::ENVIRONMENT_GET_HW_RENDER_INTERFACE => {
            warn!("TODO: Handle ENVIRONMENT_GET_HW_RENDER_INTERFACE"); // Changed from println!
            true
        }
        libretro_sys::ENVIRONMENT_GET_INPUT_DEVICE_CAPABILITIES => {
            warn!("TODO: Handle ENVIRONMENT_GET_INPUT_DEVICE_CAPABILITIES"); // Changed from println!
            true
        }
        libretro_sys::ENVIRONMENT_GET_LANGUAGE => {
            warn!("TODO: Handle ENVIRONMENT_GET_LANGUAGE"); // Changed from println!
            true
        }
        libretro_sys::ENVIRONMENT_GET_LIBRETRO_PATH => {
            warn!("TODO: Handle ENVIRONMENT_GET_LIBRETRO_PATH"); // Changed from println!
            true
        }
        libretro_sys::ENVIRONMENT_GET_LOCATION_INTERFACE => {
            warn!("TODO: Handle ENVIRONMENT_GET_LOCATION_INTERFACE"); // Changed from println!
            true
        }
        libretro_sys::ENVIRONMENT_GET_LOG_INTERFACE => {
            info!("TODO: Handle ENVIRONMENT_GET_LOG_INTERFACE"); // Changed from println!
            (*(return_data as *mut LogCallback)).log = libretro_log_print_callback;
            true
        }
        libretro_sys::ENVIRONMENT_GET_OVERSCAN => {
            warn!("TODO: Handle ENVIRONMENT_GET_OVERSCAN"); // Changed from println!
            true
        }
        libretro_sys::ENVIRONMENT_GET_PERF_INTERFACE => {
            warn!("TODO: Handle ENVIRONMENT_GET_PERF_INTERFACE"); // Changed from println!
            true
        }
        libretro_sys::ENVIRONMENT_GET_RUMBLE_INTERFACE => {
            warn!("TODO: Handle ENVIRONMENT_GET_RUMBLE_INTERFACE"); // Changed from println!
            true
        }
        libretro_sys::ENVIRONMENT_GET_SAVE_DIRECTORY => {
            warn!("TODO: Handle ENVIRONMENT_GET_SAVE_DIRECTORY");
            match unsafe { &(*(&raw const CURRENT_EMULATOR_STATE)).system_directory } {
                Some(s_dir_c) => {
                    *(return_data as *mut *const libc::c_char) = s_dir_c.as_ptr();
                    true
                }
                None => {
                    *(return_data as *mut *const libc::c_char) = ptr::null();
                    // Consider returning false if a null ptr isn't acceptable by core for this.
                    // For now, assume core handles null or we should provide a default.
                    warn!("System directory CString is None for ENVIRONMENT_GET_SAVE_DIRECTORY");
                    true 
                }
            }
        }
        libretro_sys::ENVIRONMENT_GET_SENSOR_INTERFACE => {
            warn!("TODO: Handle ENVIRONMENT_GET_SENSOR_INTERFACE");
            true
        }
        libretro_sys::ENVIRONMENT_GET_SYSTEM_DIRECTORY => {
            info!("ENVIRONMENT_GET_SYSTEM_DIRECTORY called");
            match unsafe { &(*(&raw const CURRENT_EMULATOR_STATE)).system_directory } {
                Some(s_dir_c) => {
                    *(return_data as *mut *const libc::c_char) = s_dir_c.as_ptr();
                    debug!("System directory provided: {:?}", s_dir_c);
                    true
                }
                None => {
                    *(return_data as *mut *const libc::c_char) = ptr::null();
                    warn!("System directory CString is None for ENVIRONMENT_GET_SYSTEM_DIRECTORY");
                    // Returning false might be more appropriate if the core requires a valid path.
                    // However, providing null and returning true is also a common pattern.
                    true
                }
            }
        }
        libretro_sys::ENVIRONMENT_GET_USERNAME => {
            warn!("TODO: Handle ENVIRONMENT_GET_USERNAME"); // Changed from println!
            true
        }
        libretro_sys::ENVIRONMENT_GET_VARIABLE => {
            warn!("TODO: Handle ENVIRONMENT_GET_VARIABLE command: {}", command); // Changed from println! // 15
            false
        }
        // Rest of the SET_
        libretro_sys::ENVIRONMENT_SET_DISK_CONTROL_INTERFACE => {
            warn!("TODO: Handle ENVIRONMENT_SET_DISK_CONTROL_INTERFACE"); // Changed from println!
            true
        }
        libretro_sys::ENVIRONMENT_SET_FRAME_TIME_CALLBACK => {
            warn!("TODO: Handle ENVIRONMENT_SET_FRAME_TIME_CALLBACK"); // Changed from println!
            true
        }
        libretro_sys::ENVIRONMENT_SET_GEOMETRY => {
            warn!("TODO: Handle ENVIRONMENT_SET_GEOMETRY"); // Changed from println!
            true
        }
        libretro_sys::ENVIRONMENT_SET_HW_RENDER => {
            warn!("TODO: Handle ENVIRONMENT_SET_HW_RENDER"); // Changed from println!
            true
        }
        libretro_sys::ENVIRONMENT_SET_INPUT_DESCRIPTORS => {
            warn!("TODO: Handle ENVIRONMENT_SET_INPUT_DESCRIPTORS"); // Changed from println!
            true
        }
        libretro_sys::ENVIRONMENT_SET_KEYBOARD_CALLBACK => {
            warn!("TODO: Handle ENVIRONMENT_SET_KEYBOARD_CALLBACK"); // Changed from println!
            true
        }
        libretro_sys::ENVIRONMENT_SET_MESSAGE => {
            warn!("TODO: Handle ENVIRONMENT_SET_MESSAGE"); // Changed from println!
            true
        }
        libretro_sys::ENVIRONMENT_SET_PERFORMANCE_LEVEL => {
            warn!("TODO: Handle ENVIRONMENT_SET_PERFORMANCE_LEVEL"); // Changed from println!
            true
        }
        libretro_sys::ENVIRONMENT_SET_PROC_ADDRESS_CALLBACK => {
            warn!("TODO: Handle ENVIRONMENT_SET_PROC_ADDRESS_CALLBACK"); // Changed from println!
            true
        }
        libretro_sys::ENVIRONMENT_SET_ROTATION => {
            warn!("TODO: Handle ENVIRONMENT_SET_ROTATION"); // Changed from println!
            true
        }
        libretro_sys::ENVIRONMENT_SET_SUBSYSTEM_INFO => {
            warn!("TODO: Handle ENVIRONMENT_SET_SUBSYSTEM_INFO"); // Changed from println!
            true
        }
        libretro_sys::ENVIRONMENT_SET_SUPPORT_NO_GAME => {
            warn!("TODO: Handle ENVIRONMENT_SET_SUPPORT_NO_GAME"); // Changed from println!
            true
        }
        libretro_sys::ENVIRONMENT_SET_SYSTEM_AV_INFO => {
            warn!("TODO: Handle ENVIRONMENT_SET_SYSTEM_AV_INFO"); // Changed from println!
            true
        }
        libretro_sys::ENVIRONMENT_SET_VARIABLES => {
            warn!("TODO: Handle ENVIRONMENT_SET_VARIABLES"); // Changed from println!
            true
        }
        libretro_sys::ENVIRONMENT_EXPERIMENTAL => {
            warn!("TODO: Handle ENVIRONMENT_EXPERIMENTAL"); // Changed from println!
            true
        }
        libretro_sys::ENVIRONMENT_PRIVATE => {
            warn!("TODO: Handle ENVIRONMENT_PRIVATE"); // Changed from println!
            true
        }
        libretro_sys::ENVIRONMENT_SHUTDOWN => {
            warn!("TODO: Handle ENVIRONMENT_SHUTDOWN"); // Changed from println!
            true
        }
        55 => {
            warn!("TODO: Handle RETRO_ENVIRONMENT_SET_CORE_OPTIONS_DISPLAY"); // Changed from println!
            false
        }
        66 => {
            // TODO: need to return retro_game_info_ext retro_game_info_ext
            info!("Handle ENVIRONMENT_GET_GAME_INFO_EXT");
            let game_info_ext_ptr = return_data as *mut GameInfoExt;

            unsafe {
                // Assign pointers from EmulatorState's CString fields
                (*game_info_ext_ptr).full_path = CURRENT_EMULATOR_STATE.rom_full_path_c.as_ref().map_or(ptr::null(), |cs| cs.as_ptr());
                (*game_info_ext_ptr).dir = CURRENT_EMULATOR_STATE.rom_dir_c.as_ref().map_or(ptr::null(), |cs| cs.as_ptr());
                (*game_info_ext_ptr).name = CURRENT_EMULATOR_STATE.rom_name_c.as_ref().map_or(ptr::null(), |cs| cs.as_ptr());
                (*game_info_ext_ptr).ext = CURRENT_EMULATOR_STATE.rom_ext_c.as_ref().map_or(ptr::null(), |cs| cs.as_ptr());

                // These fields are typically null if not loading from an archive directly
                (*game_info_ext_ptr).archive_path = ptr::null(); 
                (*game_info_ext_ptr).archive_file = ptr::null();
                (*game_info_ext_ptr).file_in_archive = false;

                (*game_info_ext_ptr).meta = ptr::null(); // No specific metadata for now

                if let Some(game_info_ref) = (*(&raw const CURRENT_EMULATOR_STATE)).game_info.as_ref() {
                    (*game_info_ext_ptr).data = game_info_ref.data;
                    (*game_info_ext_ptr).size = game_info_ref.size;
                } else {
                    (*game_info_ext_ptr).data = ptr::null();
                    (*game_info_ext_ptr).size = 0;
                    warn!("GameInfo was None when populating GameInfoExt for data/size");
                }
                
                (*game_info_ext_ptr).persistent_data = false; // Assuming data is not persistent unless core indicates otherwise

                debug!("GameInfoExt populated: full_path: {:?}, dir: {:?}, name: {:?}, ext: {:?}", 
                    (*game_info_ext_ptr).full_path, (*game_info_ext_ptr).dir, (*game_info_ext_ptr).name, (*game_info_ext_ptr).ext);
                debug!("GameInfoExt data size: {}", (*game_info_ext_ptr).size);
            }
            true
        }
        _ => {
            warn!( // Changed from println!
                "libretro_environment_callback Called with command: {}",
                command
            );
            false
        }
    }
}

unsafe fn load_core(library_path: &String) -> CoreAPI {
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
        info!("API Version: {}", api_version); // Changed from println!
        if api_version != EXPECTED_LIB_RETRO_VERSION {
            panic!("The Core has been compiled with a LibRetro API that is unexpected, we expected version to be: {} but it was: {}", EXPECTED_LIB_RETRO_VERSION, api_version)
        }
        (core_api.retro_set_environment)(libretro_environment_callback);
        (core_api.retro_init)();
        (core_api.retro_set_video_refresh)(libretro_set_video_refresh_callback);
        (core_api.retro_set_input_poll)(libretro_set_input_poll_callback);
        (core_api.retro_set_input_state)(libretro_set_input_state_callback);
        (core_api.retro_set_audio_sample)(libretro_set_audio_sample_callback);
        (core_api.retro_set_audio_sample_batch)(libretro_set_audio_sample_batch_callback);
        core_api
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
        _ => warn!("We don't have RetroArch config"), // Changed from println!
    }
    match our_config {
        Ok(config) => merged_config.extend(config),
        _ => warn!("We don't have RustroArch config"), // Changed from println!
    }
    // debug!("retro_arch_config_path: {} merged_config: {:?}", retro_arch_config_path.join("config/retroarch.cfg").display(), merged_config);
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
    info!("ROM name: {}", rom_name); // Changed from println!
    info!("Core Library name: {}", library_name); // Changed from println!
    CURRENT_EMULATOR_STATE.rom_name = rom_name.to_string();
    CURRENT_EMULATOR_STATE.core_name = library_name.to_string();
}

unsafe fn load_rom_file(core_api: &CoreAPI, rom_name: &String) -> bool {
    info!("Loading ROM file: {:?}", rom_name);
    let rom_path = Path::new(rom_name);

    // Populate CString fields in EmulatorState
    CURRENT_EMULATOR_STATE.rom_full_path_c = Some(CString::new(rom_name.clone()).expect("Failed to create CString for full_path"));
    
    if let Some(parent_dir) = rom_path.parent() {
        if let Some(dir_str) = parent_dir.to_str() {
            CURRENT_EMULATOR_STATE.rom_dir_c = Some(CString::new(dir_str.to_string()).expect("Failed to create CString for dir"));
        } else {
            CURRENT_EMULATOR_STATE.rom_dir_c = Some(CString::new(".").expect("Default dir CString failed")); // Default to current dir if path is weird
        }
    } else {
         CURRENT_EMULATOR_STATE.rom_dir_c = Some(CString::new(".").expect("Default dir CString failed")); // Default if no parent
    }

    if let Some(file_stem) = rom_path.file_stem() {
        if let Some(name_str) = file_stem.to_str() {
            CURRENT_EMULATOR_STATE.rom_name_c = Some(CString::new(name_str.to_string()).expect("Failed to create CString for name"));
        }
    }

    if let Some(extension) = rom_path.extension() {
        if let Some(ext_str) = extension.to_str() {
            CURRENT_EMULATOR_STATE.rom_ext_c = Some(CString::new(ext_str.to_string()).expect("Failed to create CString for ext"));
        }
    }
    
    // Use the CString from EmulatorState for GameInfo path if available, otherwise create locally (should always be available now)
    let game_info_path_ptr = CURRENT_EMULATOR_STATE.rom_full_path_c.as_ref().map_or(ptr::null(), |cs| cs.as_ptr());

    let contents = fs::read(rom_name).expect("Failed to read file");
    let data: *const c_void = contents.as_ptr() as *const c_void;
    let game_info = GameInfo {
        path: game_info_path_ptr, // Use the CString from EmulatorState
        data,
        size: contents.len(),
        meta: ptr::null(),
    };
    
    CURRENT_EMULATOR_STATE.game_info = Some(game_info.clone()); // game_info still holds ptrs from EmulatorState CStrings

    info!("INFO: Calling retro_load_game in Core");
    let was_load_successful = (core_api.retro_load_game)(&game_info);
    if !was_load_successful {
        panic!("Rom Load was not successful");
    }
    info!("ROM was successfully loaded");
    was_load_successful
}

unsafe fn send_audio_to_thread(sender: &Sender<&Vec<i16>>) {
    // Send the audio samples to the audio thread using the channel
    if let Some(data) = unsafe { &(*(&raw const CURRENT_EMULATOR_STATE)).audio_data } {
        sender.send(data).unwrap();
    };
}

unsafe fn play_audio(sink: &Sink, audio_samples: &[i16], sample_rate: u32) {
    if !AUDIO_ENABLE {
        return;
    }
    if sink.empty() {
        let audio_slice =
            std::slice::from_raw_parts(audio_samples.as_ptr(), audio_samples.len());
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
) -> PathBuf { // Return PathBuf directly
    let saves_dir = PathBuf::from(save_directory);
    // Directory creation removed from here
    let game_name = Path::new(game_file_name)
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .replace(" ", "_");
    let save_state_file_name = format!("{}_{}.state", game_name, save_state_index);
    saves_dir.join(save_state_file_name)
}

unsafe fn save_state(core_api: &CoreAPI, save_directory: &String) {
    // Create the save directory if it doesn't exist
    let saves_dir_path = PathBuf::from(save_directory);
    if !saves_dir_path.exists() {
        match std::fs::create_dir_all(&saves_dir_path) { // Use create_dir_all
            Ok(_) => info!("Created save state directory: {:?}", saves_dir_path),
            Err(e) => {
                //panic!("Failed to create save directory: {:?} Error: {}", saves_dir_path, e);
                // Log error and return, or handle as appropriate for your application
                // For now, maintaining panic to match original behavior if critical
                error!("Failed to create save state directory {:?}: {}. Save will likely fail.", saves_dir_path, e);
                // Optionally, to strictly match original panic:
                panic!("Failed to create save directory: {:?} Error: {}", saves_dir_path, e);
            }
        }
    }

    let save_state_buffer_size = (core_api.retro_serialize_size)();
    let mut state_buffer: Vec<u8> = vec![0; save_state_buffer_size];
    // Call retro_serialize to create the save state
    (core_api.retro_serialize)(
        state_buffer.as_mut_ptr() as *mut c_void,
        save_state_buffer_size,
    );
    // Call the refactored get_save_state_path, .unwrap() is no longer needed
    let file_path = get_save_state_path(
        save_directory,
        &(*(&raw const CURRENT_EMULATOR_STATE)).rom_name, // Accessing static mut safely
        (*(&raw const CURRENT_EMULATOR_STATE)).current_save_slot, // Accessing static mut safely
    );
    std::fs::write(&file_path, &state_buffer).unwrap();
    info!( 
        "Save state saved to: {} with size: {}",
        &file_path.display(),
        save_state_buffer_size
    );
}

unsafe fn load_state(core_api: &CoreAPI, save_directory: &String) {
    let file_path = get_save_state_path(
        save_directory,
        &(*(&raw const CURRENT_EMULATOR_STATE)).rom_name, // Accessing static mut safely
        (*(&raw const CURRENT_EMULATOR_STATE)).current_save_slot, // Accessing static mut safely
    );
    let mut state_buffer = Vec::new();
    match File::open(&file_path) {
        Ok(mut file) => {
            // Read the save state file into a buffer
            match file.read_to_end(&mut state_buffer) {
                Ok(_) => {
                    // Call retro_unserialize to apply the save state
                    let result = (core_api.retro_unserialize)(
                        state_buffer.as_mut_ptr() as *mut c_void,
                        state_buffer.len(),
                    );
                    if result {
                        info!("Save state loaded from: {}", &file_path.display()); // Changed from println!
                    } else {
                        error!("Failed to load save state: error code {}", result); // Changed from println!
                    }
                }
                Err(err) => error!("Error reading save state file: {}", err), // Changed from println!
            }
        }
        Err(_) => warn!("Save state file not found: {}", file_path.display()), // Changed from println!
    }
}

fn setup_key_device_map(config: &HashMap<String, String>) -> HashMap<&String, usize> {
    HashMap::from([
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
    ])
}
fn setup_joypad_device_map() -> HashMap<Button, usize> {
    HashMap::from([
        (Button::South, libretro_sys::DEVICE_ID_JOYPAD_A as usize),
        (Button::East, libretro_sys::DEVICE_ID_JOYPAD_B as usize),
        (Button::West, libretro_sys::DEVICE_ID_JOYPAD_X as usize),
        (Button::North, libretro_sys::DEVICE_ID_JOYPAD_Y as usize),
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
        (Button::DPadUp, libretro_sys::DEVICE_ID_JOYPAD_UP as usize),
        (
            Button::DPadRight,
            libretro_sys::DEVICE_ID_JOYPAD_RIGHT as usize,
        ),
        (
            Button::DPadLeft,
            libretro_sys::DEVICE_ID_JOYPAD_LEFT as usize,
        ),
        (Button::Start, libretro_sys::DEVICE_ID_JOYPAD_START as usize),
        (
            Button::Select,
            libretro_sys::DEVICE_ID_JOYPAD_SELECT as usize,
        ),
    ])
}

fn init_logger() {
    // Set the RUST_LOG environment variable if it's not already set.
    // This allows controlling log level via an environment variable.
    // Example: RUST_LOG=info ./your_app
    // Example: RUST_LOG=rustro_arch=debug ./your_app
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "info"); // Default to info level if not set
    }
    env_logger::init();
}

#[cfg(test)]
mod tests {
    use super::*; // To import functions from the outer scope
    use std::path::PathBuf;
    use std::fs::{self, File};
    use std::io::Write;
    use std::collections::HashMap; // Ensure HashMap is in scope for tests

    // Helper function to create a temporary config file for testing
    fn create_test_config_file(filename: &str, content: &str) -> PathBuf {
        let test_data_dir = PathBuf::from("test_data");
        if !test_data_dir.exists() {
            fs::create_dir_all(&test_data_dir).expect("Failed to create test_data directory");
        }
        let file_path = test_data_dir.join(filename);
        let mut file = File::create(&file_path).expect("Failed to create test file");
        file.write_all(content.as_bytes()).expect("Failed to write to test file");
        file_path
    }

    #[test]
    fn test_parse_valid_config() {
        let content = r#"
key1 = "value1"
key2 = value2
key3 = "  spaced_value  "
# Comment line
invalid_line
another_key = "another_value"
        "#;
        let file_path = create_test_config_file("valid_config.cfg", content);
        let result = parse_retroarch_config(&file_path);
        assert!(result.is_ok());
        let config_map = result.unwrap();
        assert_eq!(config_map.get("key1"), Some(&"value1".to_string()));
        assert_eq!(config_map.get("key2"), Some(&"value2".to_string()));
        assert_eq!(config_map.get("key3"), Some(&"  spaced_value  ".to_string())); // Quotes are stripped, but internal spaces preserved by current logic
        assert_eq!(config_map.get("another_key"), Some(&"another_value".to_string()));
        assert_eq!(config_map.len(), 4); // Ensure only valid key-value pairs are parsed
        fs::remove_file(file_path).expect("Failed to remove test file");
    }

    #[test]
    fn test_parse_empty_config() {
        let file_path = create_test_config_file("empty_config.cfg", "");
        let result = parse_retroarch_config(&file_path);
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
        fs::remove_file(file_path).expect("Failed to remove test file");
    }

    #[test]
    fn test_parse_malformed_lines_no_equals() {
        let content = "key1value1\nrandomtext\n  another line without equals  ";
        let file_path = create_test_config_file("malformed_config.cfg", content);
        let result = parse_retroarch_config(&file_path);
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty()); // Lines without '=' are ignored
        fs::remove_file(file_path).expect("Failed to remove test file");
    }

    #[test]
    fn test_parse_non_existent_file() {
        let file_path = PathBuf::from("test_data/non_existent_config.cfg");
        let result = parse_retroarch_config(&file_path);
        assert!(result.is_err());
        if let Err(e) = result {
            assert!(e.contains("Failed to open file"));
        }
    }

    #[test]
    fn test_parse_quoted_values() {
        let content = r#"
setting_true = "true"
setting_false = "false"
path_setting = "/usr/local/bin"
empty_quotes = ""
string_with_quotes = "\"quoted string\"" 
        "#;
        let file_path = create_test_config_file("quoted_values.cfg", content);
        let result = parse_retroarch_config(&file_path);
        assert!(result.is_ok());
        let config_map = result.unwrap();
        assert_eq!(config_map.get("setting_true"), Some(&"true".to_string()));
        assert_eq!(config_map.get("setting_false"), Some(&"false".to_string()));
        assert_eq!(config_map.get("path_setting"), Some(&"/usr/local/bin".to_string()));
        assert_eq!(config_map.get("empty_quotes"), Some(&"".to_string()));
        assert_eq!(config_map.get("string_with_quotes"), Some(&"\"quoted string\"".to_string())); // Current logic keeps internal quotes if value itself is quoted
        fs::remove_file(file_path).expect("Failed to remove test file");
    }
    
    #[test]
    fn test_get_retroarch_config_path_windows() {
        let expected_path = PathBuf::from(r"C:\Users\TestUser\AppData\Roaming\retroarch"); // Use raw string for Windows paths
        assert_eq!(
            get_retroarch_config_path_for_os("windows", None, Some(r"C:\Users\TestUser\AppData\Roaming"), None),
            expected_path
        );
    }

    #[test]
    fn test_get_retroarch_config_path_macos() {
        let expected_path = PathBuf::from("/Users/testuser/Library/Application Support/RetroArch");
        assert_eq!(
            get_retroarch_config_path_for_os("macos", Some("/Users/testuser"), None, None),
            expected_path
        );
    }

    #[test]
    fn test_get_retroarch_config_path_linux_xdg_set() {
        let expected_path = PathBuf::from("/home/testuser/.config/retroarch");
        assert_eq!(
            get_retroarch_config_path_for_os("linux", Some("/home/testuser"), None, Some("/home/testuser/.config")),
            expected_path
        );
    }

    #[test]
    #[should_panic(expected = "APPDATA environment variable not found or is invalid for Windows")]
    fn test_get_retroarch_config_path_windows_panic() {
        get_retroarch_config_path_for_os("windows", None, None, None);
    }

    #[test]
    #[should_panic(expected = "HOME environment variable not found or is invalid for macOS")]
    fn test_get_retroarch_config_path_macos_panic() {
        get_retroarch_config_path_for_os("macos", None, None, None);
    }

    #[test]
    #[should_panic(expected = "XDG_CONFIG_HOME environment variable not found or is invalid for Linux/other")]
    fn test_get_retroarch_config_path_linux_panic() {
        get_retroarch_config_path_for_os("linux", None, None, None);
    }

    // Tests for get_save_state_path
    #[test]
    fn test_get_save_state_path_basic() {
        let path = get_save_state_path(&String::from("./test_saves"), "My Game.rom", 0);
        assert_eq!(path, PathBuf::from("./test_saves/My_Game_0.state"));
    }

    #[test]
    fn test_get_save_state_path_no_extension() {
        let path = get_save_state_path(&String::from("saves"), "MyOtherGame", 15);
        assert_eq!(path, PathBuf::from("saves/MyOtherGame_15.state"));
    }

    #[test]
    fn test_get_save_state_path_with_spaces_in_dir() {
        let path = get_save_state_path(&String::from("./my save states"), "game name with spaces.core", 255);
        assert_eq!(path, PathBuf::from("./my save states/game_name_with_spaces_255.state"));
    }

    #[test]
    fn test_get_save_state_path_empty_game_name() {
        // Path::new("").file_stem() is Some("")
        // .unwrap_or_default() is ""
        // .to_string_lossy() is ""
        // .replace(" ", "_") is ""
        // so format!("{}_{}.state", "", 1) -> "_1.state"
        let path = get_save_state_path(&String::from("."), "", 1);
        assert_eq!(path, PathBuf::from("./_1.state"));
    }
    
    // Tests for convert_pixel_array_from_rgb565_to_xrgb8888
    #[test]
    fn test_convert_rgb565_to_xrgb8888_known_colors() {
        // Black (0x0000) -> R=0, G=0, B=0
        // R5: 00000 -> R8: (0<<3)|(0>>2) = 0
        // G6: 000000 -> G8: (0<<2)|(0>>3) = 0
        // B5: 00000 -> B8: (0<<3)|(0>>2) = 0
        // Expected: 0x00000000
        let black_rgb565 = [0x00, 0x00];
        let expected_black_xrgb8888 = 0x00000000;

        // White (0xFFFF) -> R=31, G=63, B=31
        // R5: 11111 (31) -> R8: (31<<3)|(31>>2) = 248 | 7 = 255
        // G6: 111111 (63) -> G8: (63<<2)|(63>>3) = 252 | 7 = 255 
        // B5: 11111 (31) -> B8: (31<<3)|(31>>2) = 248 | 7 = 255
        // Expected: 0x00FFFFFF
        let white_rgb565 = [0xFF, 0xFF];
        let expected_white_xrgb8888 = 0x00FFFFFF;

        // Red (0xF800) -> R=31, G=0, B=0
        // R5: 11111 (31) -> R8: 255
        // G6: 000000 (0)  -> G8: 0
        // B5: 00000 (0)  -> B8: 0
        // Expected: 0x00FF0000
        let red_rgb565 = [0xF8, 0x00];
        let expected_red_xrgb8888 = 0x00FF0000;
        
        // Green (0x07E0) -> R=0, G=63, B=0
        // R5: 00000 (0)  -> R8: 0
        // G6: 111110 (62 not 63, typo in prompt, 0x07E0 is G=62) -> G6: 111110 (62) -> G8: (62<<2)|(62>>3) = 248 | 7 = 255
        // Let's re-calculate for G=63 (0x07E0 is R=0, G=31, B=0 if middle 6 bits are G)
        // R(5): 00000 = 0
        // G(6): 111110 = 62. (0x07E0 -> 00000 111110 00000)
        // B(5): 00000 = 0
        // R8: 0
        // G8: (62<<2)|(62>>3) = 248 | 7 = 255
        // B8: 0
        // Expected for 0x07E0 : 0x0000FF00
        let green_rgb565 = [0x07, 0xE0]; // R=0, G=62, B=0
        let expected_green_xrgb8888 = 0x0000FF00; // Corrected calculation for G=62

        // Blue (0x001F) -> R=0, G=0, B=31
        // R5: 00000 (0)  -> R8: 0
        // G6: 000000 (0)  -> G8: 0
        // B5: 11111 (31) -> B8: 255
        // Expected: 0x000000FF
        let blue_rgb565 = [0x00, 0x1F];
        let expected_blue_xrgb8888 = 0x000000FF;

        let input_data = [
            &black_rgb565[..],
            &white_rgb565[..],
            &red_rgb565[..],
            &green_rgb565[..],
            &blue_rgb565[..],
        ].concat();
        
        let expected_output = vec![
            expected_black_xrgb8888,
            expected_white_xrgb8888,
            expected_red_xrgb8888,
            expected_green_xrgb8888,
            expected_blue_xrgb8888,
        ].into_boxed_slice();

        assert_eq!(convert_pixel_array_from_rgb565_to_xrgb8888(&input_data), expected_output);
    }

    #[test]
    fn test_convert_rgb565_empty_input() {
        assert_eq!(convert_pixel_array_from_rgb565_to_xrgb8888(&[]), vec![].into_boxed_slice());
    }

    #[test]
    #[should_panic(expected = "color_array length must be a multiple of 2 (16-bits per pixel)")]
    fn test_convert_rgb565_invalid_length() {
        convert_pixel_array_from_rgb565_to_xrgb8888(&[0x00]);
    }
}

fn main() {
    init_logger(); // Initialize the logger
    unsafe { parse_command_line_arguments() };
    let config = setup_config().unwrap();

    let key_device_map = setup_key_device_map(&config);
    let joypad_device_map = setup_joypad_device_map();

    info!("Setting up minifb window"); // Changed from println!
    let mut window =
        Window::new("RustroArch", 640, 480, WindowOptions::default()).unwrap_or_else(|e| {
            panic!("{}", e);
        });

    let mut fps_timer = Instant::now();
    let mut fps_counter = 0;
    let core_api;

    info!("Setting up Audio Thread"); // Changed from println!
    // Create a channel for passing audio samples from the main thread to the audio thread
    let (sender, receiver) = channel();

    // Spawn a new thread to play back audio
    if AUDIO_ENABLE {
        let _audio_thread = thread::spawn(move || {
            info!("Audio Thread Started"); // Changed from println!
            let sample_rate = unsafe {
                match &(*(&raw const CURRENT_EMULATOR_STATE)).av_info {
                    Some(av_info) => av_info.timing.sample_rate,
                    None => 0.0,
                }
            };
            let (_stream, stream_handle) = OutputStream::try_default().unwrap();
            let sink = Sink::try_new(&stream_handle).unwrap();
            loop {
                // Receive the next set of audio samples from the channel
                let audio_samples = receiver.recv().unwrap();
                unsafe {
                    play_audio(&sink, audio_samples, sample_rate as u32);
                }
            }
        });
    }

    info!("Gamepad Setup"); // Changed from println!
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
        info!("Setting up Core"); // Changed from println!
        core_api = load_core(&CURRENT_EMULATOR_STATE.core_name);
        (core_api.retro_init)();
        (core_api.retro_get_system_av_info)(&mut av_info);
        info!("AV Info: {:?}", &av_info); // Changed from println!
        CURRENT_EMULATOR_STATE.av_info = Some(av_info.clone());
        // Environment variables
        CURRENT_EMULATOR_STATE.system_directory = Some(CString::new("System").unwrap());

        info!("About to load ROM: {:?}", (*(&raw const CURRENT_EMULATOR_STATE)).rom_name); // Changed from println!
        load_rom_file(&core_api, &CURRENT_EMULATOR_STATE.rom_name);
    }

    let fps = av_info.timing.fps as u64;
    window.limit_update_rate(Some(std::time::Duration::from_micros(1000000 / fps)));
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
        while let Some(Event { id, event: event_type, time: _time }) = gilrs.next_event() {
            match event_type {
                EventType::Connected => {
                    info!("Gamepad connected: {:?}", id);
                    if active_gamepad.is_none() {
                        active_gamepad = Some(id);
                    }
                }
                EventType::Disconnected => {
                    info!("Gamepad disconnected: {:?}", id);
                    if active_gamepad == Some(id) {
                        active_gamepad = None;
                        // Optionally, try to find another connected gamepad
                        for (new_id, _gamepad) in gilrs.gamepads() {
                            active_gamepad = Some(new_id);
                            break;
                        }
                    }
                }
                _ => {
                    // Other event types like ButtonPressed, AxisChanged, etc. are handled below by checking state
                }
            }
             // debug!("{:?} New event from {}: {:?}", _time, id, event_type);
            if active_gamepad.is_none() { // if the current active gamepad got disconnected and no other was found
                 for (new_id, _gamepad) in gilrs.gamepads() { // check for any other connected gamepad
                    active_gamepad = Some(new_id);
                    info!("Switched active gamepad to {:?}", new_id);
                    break;
                }
            }
        }

        // You can also use cached gamepad state
        if let Some(gamepad) = active_gamepad.map(|id| gilrs.gamepad(id)) {
            for button in [
                Button::South,
                Button::North,
                Button::East,
                Button::West,
                Button::Start,
                Button::Select,
                Button::DPadDown,
                Button::DPadUp,
                Button::DPadLeft,
                Button::DPadRight,
                Button::LeftTrigger,
                Button::LeftTrigger2,
                Button::RightTrigger,
                Button::RightTrigger2,
            ] {
                if gamepad.is_pressed(button) {
                    debug!("Button Pressed: {:?}", button); // Changed from println!
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
                if key_as_string == config["input_save_state"] {
                    save_state(&core_api, &config["savestate_directory"]);
                    continue;
                }
                if key_as_string == config["input_load_state"] {
                    load_state(&core_api, &config["savestate_directory"]);
                    continue;
                }
                if key_as_string == config["input_state_slot_increase"] {
                    if unsafe { (*(&raw const CURRENT_EMULATOR_STATE)).current_save_slot } != 255 {
                        CURRENT_EMULATOR_STATE.current_save_slot += 1;
                        info!( // Changed from println!
                            "Current save slot increased to: {}",
                            unsafe { (*(&raw const CURRENT_EMULATOR_STATE)).current_save_slot }
                        )
                    }
                    continue;
                }
                if key_as_string == config["input_state_slot_decrease"] {
                    if unsafe { (*(&raw const CURRENT_EMULATOR_STATE)).current_save_slot } != 0 {
                        CURRENT_EMULATOR_STATE.current_save_slot -= 1;
                        info!( // Changed from println!
                            "Current save slot decreased to: {}",
                            unsafe { (*(&raw const CURRENT_EMULATOR_STATE)).current_save_slot }
                        )
                    }
                    continue;
                }
                warn!("Unhandled Key Pressed: {} ", key_as_string); // Changed from println!
            }

            CURRENT_EMULATOR_STATE.buttons_pressed = Some(this_frames_pressed_buttons);
            send_audio_to_thread(&sender);

            match unsafe { &(*(&raw const CURRENT_EMULATOR_STATE)).frame_buffer } {
                Some(buffer) => {
                    let width = (unsafe { (*(&raw const CURRENT_EMULATOR_STATE)).screen_pitch }
                        / unsafe { (*(&raw const CURRENT_EMULATOR_STATE)).bytes_per_pixel } as u32)
                        as usize;
                    let height = unsafe { (*(&raw const CURRENT_EMULATOR_STATE)).screen_height } as usize;
                    let slice_of_pixel_buffer: &[u32] =
                        std::slice::from_raw_parts(buffer.as_ptr() as *const u32, buffer.len()); // convert to &[u32] slice reference
                    if slice_of_pixel_buffer.len() < width * height * 4 {
                        // The frame buffer isn't big enough so lets add additional pixels just so we can display it
                        let mut vec: Vec<u32> = slice_of_pixel_buffer.to_vec();
                        // warn!("Frame Buffer wasn't big enough"); // Changed from println!
                        vec.resize(width * height * 4, 0x0000FFFF); // Add any missing pixels with colour blue
                        window.update_with_buffer(&vec, width, height).unwrap();
                    } else {
                        window
                            .update_with_buffer(slice_of_pixel_buffer, width, height)
                            .unwrap();
                    }
                }
                None => {
                    warn!("We don't have a buffer to display"); // Changed from println!
                }
            }
        }
    }
    // Cleanup at the end
}
