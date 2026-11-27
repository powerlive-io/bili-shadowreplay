The decoder clock and output frame scheduler implement the behavior described
by the official FFmpeg [6.1 decoder](https://github.com/FFmpeg/FFmpeg/blob/n6.1.3/fftools/ffmpeg_dec.c),
[9.0 decoder](https://github.com/FFmpeg/FFmpeg/blob/n9.0.2/fftools/ffmpeg_dec.c),
[6.1 filter scheduler](https://github.com/FFmpeg/FFmpeg/blob/n6.1.3/fftools/ffmpeg_filter.c),
and [9.0 filter scheduler](https://github.com/FFmpeg/FFmpeg/blob/n9.0.2/fftools/ffmpeg_filter.c).
Auto mode also follows the output policy selection in
[ffmpeg_mux_init.c](https://github.com/FFmpeg/FFmpeg/blob/n9.0.2/fftools/ffmpeg_mux_init.c).

The independent CLI oracle generates six frames, each a different color, and
encodes them as PNG packets for `framehash`. It compares frame identity, PTS,
and duration with the Rust scheduler. Its 25 combinations cover 10/25/30 fps
CFR, VFR, passthrough, gaps, initial offsets, EOF median tails, and half-tick
rounding for output-side `-ss .15 -t .35`. Duration and pixels must match
exactly. The oracle needs the CLI's `lavfi` input, `png` encoder, and
`framehash` muxer. The remaining five tests use only the libraries.

The scheduler uses C `llrint`/`llrintf` nearest-even rounding and FFmpeg's added
fractional-position precision. It keeps estimated decoder durations separate
from real frame durations. Repeated output frames hold references to pixels,
and batches are lazy so a long gap is cancellable without preallocating a large
frame list. The CLI's default excessive-duplication threshold is retained.
