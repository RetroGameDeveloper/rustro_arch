# rustro_arch

A small lightweight LibRetro Frontend written in Rust (learning project)

# Step 1 - Setup MiniFB

The first step was just to get a window where we can draw pixels and respond to user input, we want it to be very simple and cross-platform so we can use the `minifb` library.

```rust
use minifb::{Key, Window, WindowOptions};

const WIDTH: usize = 640;
const HEIGHT: usize = 480;

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

    window.limit_update_rate(Some(std::time::Duration::from_micros(16600))); // ~60fps

    let mut x: usize = 0;
    let mut y: usize = 0;

    while window.is_open() && !window.is_key_down(Key::Escape) {
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

        // Set the pixel to blue
        buffer[y * WIDTH + x] = 0x0000FFFF;

        // Update the window buffer and display the changes
        window.update_with_buffer(&buffer, WIDTH, HEIGHT).unwrap();
    }
}
```

The result of this is that it draws a blue pixel at an x,y position and you can move it around with the arrow keys, note that we don't clear the frame buffer every frame so it keeps all the previous positions as blue pixels too. The end result is you can draw blue lines on the screen.

# Step 2 - Clear the screen every frame

The line effect is cool but we should clear the screen to black every frame so that the player can just move the individual pixel aroun d the screen, you can do this by adding the following code to the start of the loop:

```rust
// Clear the buffer to black
for pixel in &mut buffer {
    *pixel = 0x00000000;
}
```

# Step 3 - Display the Frames per second

That looks great but is it efficinet to loop through the whole array every frame (60 times a second) to set every pixel to black? Probably not, but it would be good to have a way to measure this, lets display the frames per second and we can compare the speed of future changes.

To display the fps, you can use the Instant type from the std::time module to measure the time between frames. Here's an updated version of your code that displays the fps in the window title:

```rust
use minifb::{Key, Window, WindowOptions};
use std::time::{Duration, Instant};

const WIDTH: usize = 640;
const HEIGHT: usize = 480;

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
   // window.limit_update_rate(Some(std::time::Duration::from_micros(16600))); // ~60fps (commented out to get over 60fps)

    let mut x: usize = 0;
    let mut y: usize = 0;

    let mut fps_timer = Instant::now();
    let mut fps_counter = 0;

    while window.is_open() && !window.is_key_down(Key::Escape) {
        // Clear the buffer to black
        for pixel in &mut buffer {
            *pixel = 0x00000000;
        }
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
```

In this updated code, we use an Instant timer to measure the elapsed time between frames. We keep track of the number of frames rendered (fps_counter) and the time elapsed since the last fps update (fps_timer). When a second has passed, we calculate the fps and update the window title using the Window::set_title() method. Finally, we reset the fps counter and timer.

# Step 4 - Using buffer.fill instead of looping through array

Now that we can measure the frames per second we can test to see if using buffer.fill is more efficinet that looping through each pixel and setting to black, so replace the loop with:

```rust
buffer.fill(0x00000000);
```

I get slightly higher FPS with buffer.fill, but at the end of the day it is still looping over each pixel, one way we could fix this is to just set the previous pixel to black each frame at the start of the loop, like so:

```rust
while window.is_open() && !window.is_key_down(Key::Escape) {
        // Clear the previous pixel to black
        buffer[y * WIDTH + x] = 0x00000000;
```

This gets a much higher fps, of course this is not particularly useful right now as when creating a game it is unlikely that we will just update a single pixel per frame, but it is good to keep in mind for future optimizations, the less pixels we update per frame the more efficient we can be.

# Step 5 - Load a Dynamic Library (dll/dylib) from the code

All libRetro cores are compiled into platform-specific dynamic libraries (dylib on MacOSX and dll on Windows), we want to be able to call one of these functions from our code in order to get our frontend to do anything.

In order to do this we need to add the **libloading** crate as a dependency inside the **Cargo.toml** file like so:

```
[dependencies]
libloading = "0.7.0"
```

Then import the crate at the top of the file like so:

```
extern crate libloading;
```

We will create a function to load the dynamic library like so:

```
fn load_core() {
    unsafe {
        let lib = Library::new("my_library.dylib").expect("Failed to load Core");
    }
}
```

You should call the load_core function before the main game loop and if you have my_library.dylib in your current directory it will load it, otherwise it will print the string "Failed to load Core" and exit.

Note if you are on Windows make sure your core ends with `.dll`, on Linux `.so` and on MacOSX `.dylib`, the above example is for MacOSX.

You can download cores for your platform using the LibRetro BuildBot available here: [LibRetro Nightly Builds](https://buildbot.libretro.com/nightly/).

# Step 6 - Calling a function from the Core (Dynamic Library)

As an example lets call the function `retro_init` as it is one of the simplest functions (it doesn't require any parameters).

```rust
fn load_core() {
    unsafe {
        let core = Library::new("gambatte_libretro.dylib").expect("Failed to load Core");
        let retro_init: unsafe extern "C" fn() = *(core.get(b"retro_init").unwrap());
        retro_init();
    }
}

```

When running this may actually cause a Segmentation fault depending on which core you use as the function `retro_init` expects a few things to be set before executing. The fact that it caused a segmentation fault in the first place is a good sign in this case and we will fix this in the next step by providing the callback functions that it requires.

For more information about retro-init and the callback functions it requires you can checkout the guide: [Developing Cores for LibRetro](https://docs.libretro.com/development/cores/developing-cores/).

# Step 7 - Retrieving a response from the Core

Before we call the setup functions we should make sure that the core is written using a version of the LibRetro API that is compatible with what we expect.

The function `retro_api_version` is used for this purpose and at the time of current written just returns the number 1, we can call this function from the core and retrieve its value and print it to the console like so:

```rust
const EXPECTED_LIB_RETRO_VERSION: u32 = 1;

fn load_core() {
    unsafe {
        let core = Library::new("gambatte_libretro.dylib").expect("Failed to load Core");
        let retro_init: unsafe extern "C" fn() = *(core.get(b"retro_init").unwrap());
        let retro_api_version: unsafe extern "C" fn() -> libc::c_uint = *(core.get(b"retro_api_version").unwrap());
        let api_version = retro_api_version();
        println!("API Version: {}", api_version);
        if (api_version != EXPECTED_LIB_RETRO_VERSION) {
            panic!("The Core has been compiled with a LibRetro API that is unexpected, we expected version to be: {} but it was: {}", EXPECTED_LIB_RETRO_VERSION, api_version)
        }
    }
}

```

# Step 8 - Setting up the environment for the Core

Now to fix that segmentation fault error when calling `retro_init`, all we need to do it set whats called an `**Environment Callback**` function and pass it to the core. The Environment Callback function is used to allow the core to call back to the frontend to request information.

The information they can request comes in the form of a Command ID and is passed back to the core using a data buffer, so the Environment Callback takes in those two paramaters, we can implement this like so:

```rust
pub type EnvironmentCallback = unsafe extern "C" fn(command: libc::c_uint, data: *mut libc::c_void) -> bool;

unsafe extern "C" fn libretro_environment_callback(command: u32, data: *mut c_void) -> bool {
    println!("libretro_environment_callbac Called with command: {}", command);
    false
}

fn load_core() {
    unsafe {
        let core = Library::new("gambatte_libretro.dylib").expect("Failed to load Core");
        let retro_init: unsafe extern "C" fn() = *(core.get(b"retro_init").unwrap());
        let retro_api_version: unsafe extern "C" fn() -> libc::c_uint = *(core.get(b"retro_api_version").unwrap());
        let retro_set_environment: unsafe extern "C" fn(callback: EnvironmentCallback) = *(core.get(b"retro_set_environment").unwrap());
        let api_version = retro_api_version();
        println!("API Version: {}", api_version);
        if (api_version != EXPECTED_LIB_RETRO_VERSION) {
            panic!("The Core has been compiled with a LibRetro API that is unexpected, we expected version to be: {} but it was: {}", EXPECTED_LIB_RETRO_VERSION, api_version)
        }
        retro_set_environment(libretro_environment_callback);
        retro_init();
    }
}
```

If all goes well, when you run the program you will now not get a Segmentation fault (I didn't with the gameboy gambatte core) but it will also print out each call to the environment callback like so:

```rust
API Version: 1
callback_environment Called with command: 52
callback_environment Called with command: 16
callback_environment Called with command: 69
callback_environment Called with command: 65581
callback_environment Called with command: 27
callback_environment Called with command: 8
callback_environment Called with command: 70
callback_environment Called with command: 59
callback_environment Called with command: 39
callback_environment Called with command: 15
callback_environment Called with command: 65587
callback_environment Called with command: 64
```

All those integers you see in the output are **Command IDs** and you can see a full list of them if you go to the [LibRetro.h Header File](https://github.com/libretro/libretro-common/blob/master/include/libretro.h), they start with `RETRO_ENVIRONMENT_`.

For example you can see that the first value `52` is called `RETRO_ENVIRONMENT_GET_CORE_OPTIONS_VERSION` which is requesting the version of the LibRetro API that we expect future calls to be using.

We could define all these constants outselves, but after a quick google search you can see that there is already a rust library with these defined called `libretro-sys` that we can use instead.

# Step 9 - Using the types from libretro-sys cargo

We can now add the following to our `Cargo.toml` file:

```rust
libretro-sys = "0.1.1"
```

Now that we are using the `libretro-sys` library we can refactor the function a bit to use the `CoreAPI` type provided the the library and implement the rest of the functions, to look like this:

```rust
use libretro_sys::CoreAPI;

fn load_core() -> (Library, CoreAPI) {
    unsafe {
        let dylib = Library::new("gambatte_libretro.dylib").expect("Failed to load Core");
  
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
        return (dylib, core_api);
    }

```

I return the CoreAPI so we can call the functions in the rest of the code as it will be useful to call `retro_run` to render every frame inside the loop where we currently draw the blue pixel.

Here is an example of how we can call and use this new structure:

```rust
unsafe {
        let (dylib, core_api) = load_core();
        (core_api.retro_init)();
    }
```

If I am honest I only returned the dylib as I have not yet figured out Rust memory-management and if I don't return it then the library memory will be cleaned up causing the retro_init call to cause a Segmentation Fault. I could have passed in the dylib object to the function instead but I wanted to keep the dylib logic out of the main function. I will come back to this when I know more about Rust.

Since this basically leaks memory already we could change it to:

```rust
        let dylib = Box::leak(Box::new(Library::new("gambatte_libretro.dylib").expect("Failed to load Core")));
```

Then it will not need to be returned and will not cause a segmentation fault.

Although this is just temporary, in the future we will move all this into its own data structure with additional settings, if/when we add the ability to change cores on the fly.

# Step 10 - Read Command Line arguments for ROM to load

Currently we have hard-coded the dynamic library into the code but now we can write code to read both the core to load and the ROM name to load from the command line arguments.

In order to be a drop-in replacement for RetroArch we should try to use the same command Line options, which are available on their website [here](https://docs.libretro.com/guides/cli-intro/).

The use the prefix -L to specify the core to load and the default parameter is the ROM file to play.

To do this lets first create a new structure to hold the current emulator state such as the rom that is loaded and the core to use:

```rust
struct EmulatorState {
    rom_name: String,
    library_name: String,
}
```

Now lets write a function using the `clap` crate to parse the command line arguments and return them in our brand new structure:

```rust
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
```

You now need to pass a ROM file to the program in order to get past the argument parsing logic like so:

```rust
 cargo build --release && ./target/release/rustro_arch Tetris.gb -L ./gambatte_libretro.dylib
```

# Step 11 - Loading the ROM file

Now that we have the path of the ROM file to load we need to pass it to our core using the `retro_load_game` function. The function takes in a structure which the Rust `libretro-sys` crate calls `GameInfo`.

Lets look at the definition of the `GameInfo` struct:

```rust
pub struct GameInfo {
    // Path to game, UTF-8 encoded. Usually used as a reference. May be NULL if rom
    // was loaded from stdin or similar. retro_system_info::need_fullpath guaranteed
    // that this path is valid.
    pub path: *const libc::c_char,

    // Memory buffer of loaded game. Will be NULL if need_fullpath was set.
    pub data: *const libc::c_void,

    // Size of memory buffer.
    pub size: libc::size_t,

    // String of implementation specific meta-data.
    pub meta: *const libc::c_char,
}
```

To populate this we need to convert our Rust rom_name string into a `*const libc::c_char` and also open copy all the bytes from the ROM file and put it im a buffer that we can pass to the data field.

For the first part we can use Foreign Function Interface (FFI) crate, specifically the `std::ffi::CString` type to convert to a C pointer like so:

```rust
use std::ffi::{c_void, CString};

let rom_name_cptr = CString::new(rom_name).expect("Failed to create CString").as_ptr();
```

Now to load the ROM file and put all its bytes into a `*const libc::c_void` buffer, you can use the `std::fs::read` function to read the file into a `Vec <u8>`, and then use the  `as_ptr()` method to obtain a pointer to the underlying bytes.

So lets create a function to load the ROM and pass it to the libRetro core:

```rust
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
```

We can call this function just before the main game loop:

```rust
unsafe {
   let core_api = load_core(emulator_state.core_name);
   (core_api.retro_init)();
   println!("About to load ROM: {}", emulator_state.rom_name);
   load_rom_file(core_api, emulator_state.rom_name);
}
```

Note that when running the Tetris ROM with gambatte core it now prints out:

```rust
[Gambatte] Cannot dupe frames!
```

Looking in the Gambatte source code for this statement we find: [This code](https://github.com/libretro/gambatte-libretro/blob/4c64b5285b88a08b8134f6c36177fdca56d46192/libgambatte/libretro/libretro.cpp#L2412)

```rust
bool retro_load_game(const struct retro_game_info *info)
{
   bool can_dupe = false;
   environ_cb(RETRO_ENVIRONMENT_GET_CAN_DUPE, &can_dupe);
   if (!can_dupe)
   {
      gambatte_log(RETRO_LOG_ERROR, "Cannot dupe frames!\n");
      return false;
   }
```

Which highlights two things, one is that `retro_load_game` returns a boolean whether or not it succcessfully loads the ROM or not and also that we need to properly implemnent the enivironment callback so that we can return true for `RETRO_ENVIRONMENT_GET_CAN_DUPE` to get past this logic.

For the boolean return value lets read the value and exit if it was not successful:

```rust
unsafe fn load_rom_file(core_api: &CoreAPI, rom_name: String) -> bool {
    let rom_name_cptr = CString::new(rom_name.clone()).expect("Failed to create CString").as_ptr();
    let contents = fs::read(rom_name).expect("Failed to read file");
    let data: *const c_void = contents.as_ptr() as *const c_void;
    let game_info = GameInfo {
        path: rom_name_cptr,
        data,
        size: contents.len(),
        meta: ptr::null(),
    };
    let was_load_successful = (core_api.retro_load_game)(&game_info);
    if (!was_load_successful) {
        panic!("Rom Load was not successful");
    }
    return was_load_successful;
}
```

Now lets support ``RETRO_ENVIRONMENT_GET_CAN_DUPE``by changing our `libretro_environment_callback` function to check if the command is `ENVIRONMENT_GET_CAN_DUPE`, this is a good use for the rust `match` statement:

```rust
unsafe extern "C" fn libretro_environment_callback(command: u32, return_data: *mut c_void) -> bool {
  
    match command {
        ENVIRONMENT_GET_CAN_DUPE => println!("ENVIRONMENT_GET_CAN_DUPE"),
        _ => println!("libretro_environment_callback Called with command: {}", command)
    }
    false
}
```

This will print `ENVIRONMENT_GET_CAN_DUPE` when the command comes in but it will still not get past the logic in Gambatte as we need to return the value true into the `return_data` buffer. To do this we can use the c-like syn tax to set the dereferenced pointer to the boolean true value like so:

```rust
*(return_data as *mut bool) = true; // Set the return_data to the value true
```

On a side note I have not yet found out what exactly ``RETRO_ENVIRONMENT_GET_CAN_DUPE`` is for, apparently GameBoy generates two identical frames back-to-back, so apparently the frontend needs to support being able to duplicate the same frame in order to maintain timing.

So we now have the environment callback function like so:

```rust
unsafe extern "C" fn libretro_environment_callback(command: u32, return_data: *mut c_void) -> bool {
  
    match command {
        ENVIRONMENT_GET_CAN_DUPE => {
            *(return_data as *mut bool) = true; // Set the return_data to the value true
            println!("Set ENVIRONMENT_GET_CAN_DUPE to true");
        },
        _ => println!("libretro_environment_callback Called with command: {}", command)
    }
    false
}
```

This gets past the dupe frames error but still fails on ROM load with the message:

```rust
[Gambatte] RGB565 is not supported.
```

Again looking at the Gambatte source code we can find out where it fails [here](https://github.com/libretro/gambatte-libretro/blob/4c64b5285b88a08b8134f6c36177fdca56d46192/libgambatte/libretro/libretro.cpp#L2502) so we need to implement the `RETRO_ENVIRONMENT_SET_PIXEL_FORMAT` command too, returning true is enough to get past this check for now, but in the near future we will need to save the pixel format when we want to draw the buffer to the screen:

```rust
unsafe extern "C" fn libretro_environment_callback(command: u32, return_data: *mut c_void) -> bool {
  
    match command {
        libretro_sys::ENVIRONMENT_GET_CAN_DUPE => {
            *(return_data as *mut bool) = true; // Set the return_data to the value true
            println!("Set ENVIRONMENT_GET_CAN_DUPE to true");
        },
        libretro_sys::ENVIRONMENT_SET_PIXEL_FORMAT => {
            println!("TODO: Handle ENVIRONMENT_SET_PIXEL_FORMAT when we start drawing the the screen buffer");
            return true;
        }
        _ => println!("libretro_environment_callback Called with command: {}", command)
    }
    false
}
```

After this change Gambatte gets pretty far in loading the ROM which we can see by looking at the console messages:

```rust
TODO: Set ENVIRONMENT_SET_PIXEL_FORMAT to something
libretro_environment_callback Called with command: 9
[Gambatte] No system directory defined, unable to look for 'gbc_bios.bin'.
libretro_environment_callback Called with command: 15
[Gambatte] Plain ROM loaded.
[Gambatte] rambanks: 0
[Gambatte] rombanks: 2
[Gambatte] Got internal game name: TETRIS.
libretro_environment_callback Called with command: 15
libretro_environment_callback Called with command: 65578
```

I am going to ignore the `gbc_bios.bin` error message for now, Tetris isn't a GBC game and I believe the BIOS is optional for this emulator anyway.

# Step 12 - Running the core with retro_run

Lets now see what happens when we request the core to run a whole frame of emulation, we can do this with the `retro_run` function like so:

```rust
 unsafe {
        let core_api = load_core(emulator_state.core_name);
        (core_api.retro_init)();
        println!("About to load ROM: {}", emulator_state.rom_name);
        load_rom_file(&core_api, emulator_state.rom_name);
        (core_api.retro_run)();
    }
```

Unfortunately this causes a segmentation fault as soon as we call it without printing anything new to the console:

```rust
ROM was successfully loaded
[1]    63265 segmentation fault  ./target/release/rustro_arch Tetris.gb -L ./gambatte_libretro.dylib
```

Bare in mind that so far we have been implementing the bare minimum of the libRetro API to get to this point, so it is likely it is requesting something we have not yet implemented. So lets have a look at what [libretro.h](https://github.com/libretro/libretro-common/blob/master/include/libretro.h) says is guarantted to be called before retro_run:

```rust
/* Sets callbacks. retro_set_environment() is guaranteed to be called
 * before retro_init().
 *
 * The rest of the set_* functions are guaranteed to have been called
 * before the first call to retro_run() is made. */
RETRO_API void retro_set_environment(retro_environment_t);
RETRO_API void retro_set_video_refresh(retro_video_refresh_t);
RETRO_API void retro_set_audio_sample(retro_audio_sample_t);
RETRO_API void retro_set_audio_sample_batch(retro_audio_sample_batch_t);
RETRO_API void retro_set_input_poll(retro_input_poll_t);
RETRO_API void retro_set_input_state(retro_input_state_t);
```

We have already implemented the environment callback, but lets create dummy implementations for each of the others so we can be sure that it isn't one of these missing functions causing the segmentation fault.

First create the dummy callback functions at the top of the file:

```rust
unsafe extern "C" fn libretro_set_video_refresh_callback(data: *const libc::c_void, width: libc::c_uint, height: libc::c_uint, pitch: libc::size_t) {
    println!("libretro_set_video_refresh_callback")
}

unsafe extern "C" fn libretro_set_input_poll_callback() {
    println!("libretro_set_input_poll_callback")
}

unsafe extern "C" fn libretro_set_input_state_callback(port: libc::c_uint, device: libc::c_uint, index: libc::c_uint, id: libc::c_uint) -> i16 {
    println!("libretro_set_input_state_callback");
    return 1;
}

unsafe extern "C" fn libretro_set_audio_sample_callback(left: i16, right: i16) {
    println!("libretro_set_audio_sample_callback");
}

unsafe extern "C" fn libretro_set_audio_sample_batch_callback(data: *const i16, frames: libc::size_t) -> libc::size_t {
    println!("libretro_set_audio_sample_batch_callback");
    return 1;
}
```

As these are dummy functions we just print the function name that was called and if it requires a return value we just return the number 1, we will find out what we need to implement these later on.

Now pass them to the core after the call to `retro_init` like so:

```rust
(core_api.retro_init)();
(core_api.retro_set_video_refresh)(libretro_set_video_refresh_callback);
(core_api.retro_set_input_poll)(libretro_set_input_poll_callback);
(core_api.retro_set_input_state)(libretro_set_input_state_callback);
(core_api.retro_set_audio_sample)(libretro_set_audio_sample_callback);
(core_api.retro_set_audio_sample_batch)(libretro_set_audio_sample_batch_callback);
```

Now run the program and success it doesn't cause a segmentation fault! Lets now move the `retro_run` call into the main game loop so it calls the core every frame:

```rust
 unsafe {
        let core_api = load_core(emulator_state.core_name);
        (core_api.retro_init)();
        println!("About to load ROM: {}", emulator_state.rom_name);
        load_rom_file(&core_api, emulator_state.rom_name);
    }
  
    window.limit_update_rate(Some(std::time::Duration::from_micros(16600))); // Limit to ~60fps

    while window.is_open() && !window.is_key_down(Key::Escape) {

        // Call the libRetro core every frame
        unsafe {
            (core_api.retro_run)();
        }
```

Excellent so we can now run the core every frame and you will see a lot of lines printed to the console where it calls our callback functions such as:

```rust
libretro_set_audio_sample_batch_callback
libretro_environment_callback Called with command: 17
libretro_set_input_poll_callback
libretro_set_input_state_callback
```

# Step 13 - Get the pixel buffer from the core

Now that we have the core running it would be nice to actually see what the emulator is doing, for that we need to get the pixel buffer and display it instead of our moving blue pixel.

To get the pixel buffer from the libretro core we need to properly implement the `libretro_set_video_refresh_callback` we just created a dummy for as it is called every frame when the core has finished writing all the pixels to the frame buffer. 

The width and height parameter will be useful as it is the size of the frame in pixels, but I need to find out what the pitch variable is used for. You can print out the values every frame like so:

```rust
unsafe extern "C" fn libretro_set_video_refresh_callback(frame_buffer_data: *const libc::c_void, width: libc::c_uint, height: libc::c_uint, pitch: libc::size_t) {
    println!("libretro_set_video_refresh_callback, width: {}, height: {}, pitch: {}", width, height, pitch)
}
```

For the gambatte core it is currently printing this out every frame:

```rust
libretro_set_video_refresh_callback, width: 160, height: 144, pitch: 512
```

So the width and height look correct but lets quickly find out what pitch is and why it is set to 512, I decided to do the mordern thing ans asked ChatGPT, we got the following response:

>  In the context of libretro, pitch refers to the number of bytes between two vertically adjacent pixels in an image. It is also sometimes called the "stride" or "line stride".
>
> The pitch value is important because many image processing algorithms and hardware acceleration APIs require that images be stored in memory in a specific format with a specific pitch value. If an image's pitch value does not match the expected value, it can cause visual artifacts or errors in processing.

It gave a better explanation than my google seach, but 512 pixels between two vertical pixels seems like quite a lot, we will come back to this soon, but lets at least see what the frame_buffer looks like.

The `frame_buffer_data` parameter contains all the pixel data to display, so lets at least print it out to the console to see what we are dealing with:

```rust
unsafe extern "C" fn libretro_set_video_refresh_callback(frame_buffer_data: *const libc::c_void, width: libc::c_uint, height: libc::c_uint, pitch: libc::size_t) {
    println!("libretro_set_video_refresh_callback, width: {}, height: {}, pitch: {}", width, height, pitch);
    let length_of_frame_buffer = width*height;
    let slice = std::slice::from_raw_parts(frame_buffer_data as *const u8, length_of_frame_buffer as usize);
    println!("Frame Buffer: {:?}", slice);
}
```

This runs for a little bit and then causes a segmentation fault, if we remove the println then it will run successfully, so presumably either the frame buffer memory is being deleted while we are printing it or the `frame_buffer_data` is being passed as a null pointer, both could cause the segmentation fault.

First lets check if `frame_buffer_data` is a null pointer and return if it is:

```rust
unsafe extern "C" fn libretro_set_video_refresh_callback(frame_buffer_data: *const libc::c_void, width: libc::c_uint, height: libc::c_uint, pitch: libc::size_t) {
    if (frame_buffer_data == ptr::null()) {
        println!("frame_buffer_data was null");
        return;
    }
    println!("libretro_set_video_refresh_callback, width: {}, height: {}, pitch: {}", width, height, pitch);
    let length_of_frame_buffer = width*height;
    let slice = std::slice::from_raw_parts(frame_buffer_data as *const u8, length_of_frame_buffer as usize);
    println!("Frame Buffer: {:?}", slice);
}
```

This fixes the segmentation fault and highlights a piece of useful information, that the frame_buffer_data is sometimes null, this could be related to the dupe frames mentioned earlier, maybe if it is null it expects the frontend to just display the previous frame?


# Step 14 - Displaying the Pixel Buffer to the screen

Now we have a buffer of pixels from the core, we need to figure out how we can display them to the screen, we have two problems to solve:

* We got the buffer of pixels in our callback function but how do we get that data into the main game loop to draw to our screen?
* We need to figure out the format that the pixel buffer is in, e.g how many bytes represent red, green, blue etc and is there alpha (transparency) information in the format?

For the first problem all I can think of is creating a global variable which we can access in both the callback function and in the main game loop, there is probably a much better way to do this in rust as global variables are generally bad practise but it will do for now. Maybe at the end of this project when I know more rust I can go back and refactor the code with explanations of why.

We can use our existing struct called `EmulatorState` for the global variable and add an optional frame buffer into the definiton, in rust you can create an optional field like so:

```rust
struct EmulatorState {
    rom_name: String,
    core_name: String,
    frame_buffer: Option<Vec<u8>>,
}

static mut CURRENT_EMULATOR_STATE: EmulatorState = EmulatorState {
    rom_name: String::new(),
    core_name: String::new(),
    frame_buffer: None,
}

```

Now before we initialise the core lets set this global variable to have the current rom name and core name and an empty `frame_buffer` so lets change this previous line:

```rust
    let emulator_state = parse_command_line_arguments();
```

To instead use the global variable:

```rust
unsafe { CURRENT_EMULATOR_STATE = parse_command_line_arguments() };
```

Note that the unsafe block is required as we are modifying global state, which is not thread safe, exactly why we shouldn't be using a global variable, maybe we could put the libRetro callback as a closure inside the main function along with the variable, but that wouldn't work as the callback needs to be marked as `extern` for the core to call it, anyway lets see if we can get the pixel buffer from the callback first.


Lets set the frame_buffer on our global variable:

```rust
unsafe extern "C" fn libretro_set_video_refresh_callback(frame_buffer_data: *const libc::c_void, width: libc::c_uint, height: libc::c_uint, pitch: libc::size_t) {
    if (frame_buffer_data == ptr::null()) {
        println!("frame_buffer_data was null");
        return;
    }
    println!("libretro_set_video_refresh_callback, width: {}, height: {}, pitch: {}", width, height, pitch);
    let length_of_frame_buffer = width*height;
    let buffer_slice = std::slice::from_raw_parts(frame_buffer_data as *const u8, length_of_frame_buffer as usize);

    // Create a Vec<u8> from the slice
    let buffer_vec = Vec::from(buffer_slice);

    // Wrap the Vec<u8> in a Some Option and assign it to the frame_buffer field
    CURRENT_EMULATOR_STATE.frame_buffer = Some(buffer_vec);
    println!("Frame Buffer: {:?}", CURRENT_EMULATOR_STATE.frame_buffer);
}
```

Excellent so the frame_buffer has been successfully set on the global variable we should be able to access it from the main game loop!


So lets replace the old code that we were using to display the moving blue pixel example, from this:

```rust
window.update_with_buffer(&buffer, WIDTH, HEIGHT).unwrap();
```

To this:

```rust
unsafe {
        match &CURRENT_EMULATOR_STATE.frame_buffer {
            Some(buffer) => {
                // Do something with buffer
                let slice_u32: &[u32] = unsafe {
                    std::slice::from_raw_parts(buffer.as_ptr() as *const u32, buffer.len() / 4)
                }; // convert to &[u32] slice reference
                window.update_with_buffer(slice_u32, WIDTH, HEIGHT).unwrap();
            }
            None => {
                // Handle the case where frame_buffer is None
                println!("We don't have a buffer to display");
            }
        }
    }
```

Since the frame_buffer is optional we need to handle that using the common rust patten of using a match statement.

The `update_with_buffer` functionneed to take a u32 array but our buffer was a u8 array so we convert it and then pass it to the function.

Bare in mind we are just presuming (incorrectly) that the pixel format returned by the core will match exactly what the `minifb` library expects. So we are expecting this to put nonsense on the screen until we convert the pixel format from the core to match what `minifb` expects.

But first lets run and we realise that we get this error:

```rust
Update failed because input buffer is too small. Required size for 640 (640 stride) x 480 buffer is 1228800\n            bytes but the size of the input buffer has the size 23040 bytes
```

We are only passing 23040 bytes because we multipiled the width and height together and presumed that each pixel was a single byte which is of course incorrect.


But just to get something to display on the screen based on thsi frame buffer lets do a little hack and just fill up the rest of the buffer with the value 0 (black) like so:

```rust
unsafe {
        match &CURRENT_EMULATOR_STATE.frame_buffer {
            Some(buffer) => {
                // Do something with buffer
                let slice_u32: &[u32] = unsafe {
                    std::slice::from_raw_parts(buffer.as_ptr() as *const u32, buffer.len() / 4)
                }; // convert to &[u32] slice reference
                // Temporary hack jhust to display SOMETHING on the screen
                let mut vec: Vec<u32> = slice_u32.to_vec();
                vec.resize( 1228800, 0);
                window.update_with_buffer(&vec, WIDTH, HEIGHT).unwrap();
            }
            None => {
                // Handle the case where frame_buffer is None
                println!("We don't have a buffer to display");
            }
        }
    }
```



# Step 15 - Handling the core Pixel Format

ok lets handle the Pixel format correctly.
