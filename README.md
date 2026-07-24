A hobby Game Boy (DMG) emulator.

## AI Statement
No AI was used for any part of this project (designing, researching, or developing).

## Usage
Build:
```bash
cargo run -- <path to DMG boot ROM> <path to game ROM>
```

You can source boot ROMs and game ROMs elsewhere.
This emulator is currently able to run Tetris (1989) enough to reach the demo game
and then fails out.

TODO:
- [ ] Joypad input
- [ ] Fix tons of instruction errors
- [ ] Memory bank switching
- [ ] RNG
- [ ] Audio

References:
- `https://github.com/Gekkio/gb-ctr`
- `https://www.devrs.com/gb/files/gbspec.txt`
- `https://gbdev.io/pandocs/About.html`
- `https://www.youtube.com/watch?v=HyzD8pNlpwI`
