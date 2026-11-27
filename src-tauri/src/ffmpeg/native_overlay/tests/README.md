`translucent.png` is a deterministic 16×16, 8-bit RGBA PNG with every pixel
set to `[224, 48, 96, 128]`. Generating it requires no external image tool or
media decoder. The file contains only an IHDR chunk, one zlib-compressed IDAT
chunk, and an IEND chunk.

The native overlay tests combine this half-transparent image with text fill,
outline and shadow over black, white and coloured backgrounds. They retain the
preview comparison's existing two-level rounding tolerance and separately
compare all output BGRA bytes with a real, matching FFmpeg 6/9 CLI oracle.
