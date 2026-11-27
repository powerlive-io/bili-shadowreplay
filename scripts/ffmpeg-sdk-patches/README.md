# FFmpeg Vulkan encoder cleanup

FFmpeg 9.0.2 can leave the last queued H.264/HEVC Vulkan picture's input and DPB
image views alive at device destruction. Draining the encoder does not release
every retained reference picture. This can report `VUID-vkDestroyDevice-device-05137` independently of Skia.

The included patch is the unchanged upstream fix
[`ddfa842088cd86bd48bc4533f10a4b5badcbd02d`](https://github.com/FFmpeg/FFmpeg/commit/ddfa842088cd86bd48bc4533f10a4b5badcbd02d).
It releases queued picture views before the base context discards their records,
and releases the packet buffer pool before the base context. The application
continues to use public FFmpeg APIs; it does not inspect private picture layouts.
This backport targets the unpatched `n9.0.2` source; verify whether a newer SDK
already contains the upstream fix before applying it.

## Backport to a source build

Extract an unmodified FFmpeg 9.0.2 source tree with LF line endings, then run:

```sh
python scripts/ffmpeg-sdk-patches/apply.py /path/to/ffmpeg-9.0.2 --check
python scripts/ffmpeg-sdk-patches/apply.py /path/to/ffmpeg-9.0.2
```

The offline helper checks the SHA-256 values of the patch, the original source,
and the patched files against `manifest.json`. It rejects unrecognized source
files. Running it again after a successful patch makes no further changes.
It only patches source: building a replacement SDK still requires the intended
compiler, the full FFmpeg configuration, and all third-party build dependencies.

Rebuild and validate the complete SDK before using it for exports. Swapping
only the official `avcodec` DLL for a custom build is untested: a different
configuration can omit encoders or decoders, or introduce incompatible
dependencies. Keep matching headers, import libraries, and runtime DLLs
together in a separate prefix.
