The emoji normalizer accepts one local regular file, at most 8 MiB, identified
by PNG/APNG, JPEG, GIF or WebP magic. It feeds a bounded memory buffer to FFmpeg
through custom AVIO without an input URL. The former format whitelist and file
protocol whitelist remain in force; playlists, URLs and filename sequences
cannot supply additional inputs. Stream discovery does not run an implicit
unrestricted decoder before the image decoder's limits are installed.

The source canvas and each decoded frame are limited to 4,194,304 pixels. The
decoder, filter graph, and PNG encoder each use one thread. The normalizer
scales the first frame to fit within 512×512 without enlarging it, then encodes
it as an RGBA PNG. It publishes the temporary file atomically on success; an
error, timeout, or cancellation leaves the previous output intact. Animated
WebP that requires a demuxer outside the legacy whitelist remains unsupported;
the application can fall back to text.

PNG chunk lengths are checked against the encoded input. Compressed ICC, text
and international-text metadata are validated with a bounded streaming zlib
decoder before FFmpeg can expand them with its unlimited metadata buffer. Their
combined expansion is limited to 8 MiB; this is a stricter normalization policy
than the former command. Normal ICC data and original input chunks are retained.

The normalizer runs inside the application process. It leaves FFmpeg's
`av_max_alloc` unchanged because that setting affects the entire process. The
input, dimension, metadata, and thread limits bound the work; they are **not a
hard per-job or process memory limit**. The ten-second timeout cancels the
future, interrupts cooperative input work, and prevents publication. It cannot
forcibly stop a native codec or filter call already in progress. A native
crash or allocation failure is not isolated in a separate process.
