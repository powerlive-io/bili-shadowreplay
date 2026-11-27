Structured media plans select input streams, copy or encode them, and configure
output containers. They contain no process arguments. File jobs and shared
complex graphs support explicit stream routes, generated color/silence, bounded
raw video and multiple outputs. Each filter input binds one selected stream;
direct AllAudio routes expand in input order without another probe.

Input-side seeking and output-side trimming have separate policies. Copied
streams retain keyframe preroll; encoded streams pass through trim filters and
the native decoder/output clocks. Audio uses the same packet/sample clock and
frame queue as the fixed audio profiles. Video handles automatic rotation,
pixel-format negotiation, format changes, frame synchronization, encoder delay
and complete draining. Muxing retains stream metadata, chapters, dispositions,
codec tags and side data. Missing copied DTS uses the demux-side estimate from
fftools; coarse audio packet clocks use av_rescale_delta.

Audio encoding owns format negotiation, sample framing, duration cutoff and
encoder draining independently of file decoding. File and graph callers can
feed the same encoder stage directly after their respective filter sinks.

The shared scheduler decodes an input only when a buffer source requests it.
Other streams keep their packets compressed while they wait. Compressed queues
are limited to 64 MiB or 16384 packets, and decoded queues to 64 MiB or 1024
frames. These limits do not cap memory allocated by codecs or filters.

The scheduler handles input, graph, and encoder EOF separately. It closes all
outputs before publication and checks for predictable errors before renaming
files. Each rename is atomic; publishing multiple files is not a filesystem
transaction. Segment callers keep their partial-file guards. Resource leases
stay alive after the worker finishes so scratch directories remain available
through publication.

Shared graphs are configured once: rebuilding them midstream would reset
concat/xfade timelines and discard buffered frames. They reject changes to
input formats or rotation transforms before submitting frames to libavfilter.
Forwarding changed dimensions is unsafe even when scale is present: pad,
transpose and other filters retain their startup dimensions. Output frames must
also match the encoder's fixed format. Failures do not publish partial output.
Callers must normalize changing inputs separately. Whole-recording exports use
independent video encodes and lossless PCM audio in temporary MOV files, then
copy video and encode a single AAC stream at the join. This avoids inserting
per-segment AAC pre-roll. Independent encodes retain their own format-change
handling without resetting a composition.

The encoder stage validates AVOptions against the linked codec and rejects
unused options. To check hardware readiness, it opens the configured encoder,
feeds it three synthetic frames, and drains it. A registry entry alone is not
enough. The application keeps its candidate order and software fallback.
VAAPI devices and filter hardware frame pools are attached through libavutil.

The worker owns the input interrupt, codec/filter contexts and temporary output.
Dropping the future signals cooperative cancellation. The worker propagates
errors from writing the trailer, flushing buffered writes, and closing the
output. On success it publishes the file atomically before the final progress
callback. Progress comes from media timestamps, not from subprocess stderr.

Native FFmpeg warnings and errors also reach the application logger with their
codec/filter context. Each blocking job retains its first eight and last eight
distinct consecutive error messages, including the cause preceding teardown
failures, and appends them to returned errors. Individual messages are capped
at 4096 bytes with an explicit truncation marker. Job captures are isolated by
thread and cleared on success, failure and panic. Messages from FFmpeg-owned
codec threads reach the log but are not attributed to another worker's job.

Tests compare stream headers, metadata, dispositions and every packet with the
original single-input and complex-graph commands on FFmpeg 6 and 9. The frame
clock's exact CLI oracle is documented in video_clock/README.md. This module
does not parse arbitrary FFmpeg CLI commands or fall back to a process.
