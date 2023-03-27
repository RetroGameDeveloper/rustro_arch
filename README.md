# rustro_arch
A small lightweight LibRetro Frontend written in Rust (learning project)

# Step 1 - Setup MiniFB
The first step was just to get a window where we can draw pixels and respond to user input, we want it to be very simple and cross-platform so we can use the `minifb` library.

```
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