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
