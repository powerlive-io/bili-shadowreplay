`seek.mp4` is a generated 11-frame H.264 test pattern with B-frames: 96×64,
10 fps, SAR 4:3, and BT.709 limited range. It contains no third-party content.
Tests read the fixture directly to check decoding, accurate seeking, and JPEG
encoding; they do not need a CLI or an H.264 encoder. The optional comparison
tests use the CLI matching the linked SDK to compare JPEGs byte for byte.

Reproduce with:

```sh
ffmpeg -f lavfi -i testsrc2=size=96x64:rate=10:duration=1.1 \
  -vf setsar=4/3 -c:v libx264 -preset medium -crf 18 -g 10 -bf 3 \
  -color_range tv -colorspace bt709 -color_primaries bt709 -color_trc bt709 \
  -y seek.mp4
```
