`subtitle-black.mp4` pairs 1.1 s of generated black video (160×90, 10 fps,
H.264 with B-frames) with a generated 440 Hz mono AAC tone. It contains no
third-party media. Native libass rendering tests use it to check color, style
overrides, and cue boundaries, and to verify that the audio stays unchanged.
Running those tests does not require a CLI.

Reproduce with:

```sh
ffmpeg -f lavfi -i color=black:size=160x90:rate=10:duration=1.1 \
  -f lavfi -i sine=frequency=440:sample_rate=48000:duration=1.1 \
  -c:v libx264 -crf 18 -g 10 -bf 2 -c:a aac -b:a 64k -y subtitle-black.mp4
```
