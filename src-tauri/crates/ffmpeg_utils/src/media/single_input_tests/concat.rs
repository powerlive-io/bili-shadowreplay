use super::*;

fn concat_plan(list: &Path, output: &Path) -> MediaPlan {
    let mut request = plan(list, output, StreamMode::Copy, StreamMode::Copy);
    request.inputs[0].format = Some("concat".into());
    request.inputs[0].options.insert("safe".into(), "0".into());
    request.routes.push(StreamRoute {
        source: StreamSource::Input {
            input: 0,
            selector: StreamSelector::AutoSubtitle,
        },
        output: 0,

        required: false,
        mode: StreamMode::Copy,
    });
    request
}

fn file_entry(path: &Path) -> String {
    format!(
        "file '{}'\n",
        path.to_str()
            .unwrap()
            .replace('\\', "\\\\")
            .replace('\'', "'\\''")
    )
}

#[tokio::test]
async fn copy_concat_preroll_normalization_preserves_packets_and_allows_opus() {
    if !has_cli() {
        return;
    }
    for rate in [44_100, 48_000] {
        assert_copy_concat_preroll(rate).await;
    }
}

async fn assert_copy_concat_preroll(rate: u32) {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source.ts");
    cli(
        &strings(&[
            "-f",
            "lavfi",
            "-i",
            "color=c=blue:s=160x90:r=30:d=42",
            "-f",
            "lavfi",
            "-i",
            &format!("sine=sample_rate={rate}:duration=42"),
            "-c:v",
            "libx264",
            "-threads:v",
            "1",
            "-bf",
            "3",
            "-g",
            "60",
            "-keyint_min",
            "60",
            "-sc_threshold",
            "0",
            "-c:a",
            "aac",
        ]),
        &source,
    );
    // The CLI only creates the fixture. Remux, input seeking, duration limits,
    // concat and sidecar extraction use the application's native media paths.
    let remuxed = directory.path().join("source.mp4");
    execute_media_plan(
        plan(&source, &remuxed, StreamMode::Copy, StreamMode::Copy),
        None,
    )
    .await
    .unwrap();
    let mut list = String::new();
    // The reported ranges, shifted by whole GOPs into a short synthetic source.
    for (index, (start, end)) in [(5.22, 11.67), (16.8, 31.32), (32.61, 39.2559621565705)]
        .into_iter()
        .enumerate()
    {
        let cut = directory.path().join(format!("cut-{index}.mp4"));
        let mut request = plan(&remuxed, &cut, StreamMode::Copy, StreamMode::Copy);
        request.inputs[0].seek = Some(InputSeek {
            seconds: start,
            accurate: true,
        });
        request.outputs[0].duration = Some(end - start);
        execute_media_plan(request, None).await.unwrap();
        crate::encode_opus_sample(&cut, &cut.with_extension("opus"))
            .await
            .unwrap();
        list.push_str(&file_entry(&cut));
    }
    let filelist = directory.path().join("list.txt");
    fs::write(&filelist, list).unwrap();
    let joined = directory.path().join("joined.mp4");
    execute_media_plan(concat_plan(&filelist, &joined), None)
        .await
        .unwrap();
    // Negative preroll from the next cut overlaps the previous cut. The mux
    // clamp produces increasing DTS, but cannot shorten the AAC sample blocks.
    let before = snapshot(&joined);
    let audio = before
        .streams
        .iter()
        .find(|s| s.kind == media::Type::Audio)
        .unwrap();
    assert!(audio.packets.iter().any(|packet| packet.duration == 1));
    let failure = crate::encode_opus_sample(&joined, &joined.with_extension("opus")).await;
    // Report the original failure without requiring it forever: a future
    // sidecar fix may accept the same overlapping MP4, but must not weaken the
    // packet-preservation checks on the copy-only alternative below.
    eprintln!("{rate} Hz, original copy sidecar: {failure:?}");
    if let Err(error) = failure {
        assert!(error.contains("写入音频编码包失败"), "{error}");
        assert!(!joined.with_extension("opus").exists());
    }

    let mut normalized_list = String::new();
    let mut normalized_movies = Vec::new();
    for index in 0..3 {
        let cut = directory.path().join(format!("cut-{index}.mp4"));
        let normalized = directory.path().join(format!("normalized-{index}.mp4"));
        // Equivalent to -c copy -avoid_negative_ts make_zero on EACH cut,
        // before concat has clamped away the overlapping timestamps.
        let mut request = plan(&cut, &normalized, StreamMode::Copy, StreamMode::Copy);
        request.outputs[0].negative_ts = NegativeTs::MakeZero;
        execute_media_plan(request, None).await.unwrap();
        let original = snapshot(&cut);
        let shifted = snapshot(&normalized);
        let mut common_shift = None::<f64>;
        for (original, shifted) in original.streams.iter().zip(&shifted.streams) {
            assert_eq!(original.codec, shifted.codec);
            assert_eq!(original.time_base, shifted.time_base);
            assert_eq!(original.packets.len(), shifted.packets.len());
            let offset = shifted.packets[0].dts.unwrap() - original.packets[0].dts.unwrap();
            let seconds = offset as f64 * original.time_base.0 as f64 / original.time_base.1 as f64;
            if let Some(previous) = common_shift {
                assert!((seconds - previous).abs() < 1.0 / f64::from(rate));
            }
            common_shift = Some(seconds);
            for (original, shifted) in original.packets.iter().zip(&shifted.packets) {
                assert_eq!(
                    original.payload, shifted.payload,
                    "copy must not change coded content"
                );
                assert_eq!(shifted.pts.unwrap() - original.pts.unwrap(), offset);
                assert_eq!(shifted.dts.unwrap() - original.dts.unwrap(), offset);
            }
        }
        normalized_movies.push(shifted);
        normalized_list.push_str(&file_entry(&normalized));
    }
    let normalized_filelist = directory.path().join("normalized-list.txt");
    fs::write(&normalized_filelist, normalized_list).unwrap();
    let normalized_join = directory.path().join("normalized-joined.mp4");
    execute_media_plan(concat_plan(&normalized_filelist, &normalized_join), None)
        .await
        .unwrap();
    let after = snapshot(&normalized_join);
    for (stream_index, stream) in after.streams.iter().enumerate() {
        let originals: Vec<_> = normalized_movies
            .iter()
            .flat_map(|movie| &movie.streams[stream_index].packets)
            .collect();
        assert_eq!(stream.packets.len(), originals.len());
        for (actual, original) in stream.packets.iter().zip(originals) {
            if stream.kind == media::Type::Video {
                assert_eq!(
                    coded_nals(actual, &stream.extradata),
                    coded_nals(original, &stream.extradata)
                );
            } else {
                assert_eq!(actual.payload, original.payload);
            }
        }
        assert!(stream.packets.windows(2).all(|p| p[1].dts > p[0].dts));
        if stream.kind == media::Type::Audio {
            // Allow a sample of rational rounding, not a whole decoded frame
            // squeezed into one timestamp tick at a join.
            assert!(stream
                .packets
                .windows(2)
                .all(|p| p[1].pts.unwrap() - p[0].pts.unwrap() >= 1023));
        }
        let duration =
            stream.duration as f64 * stream.time_base.0 as f64 / stream.time_base.1 as f64;
        eprintln!(
            "{rate} Hz, normalized {:?} duration: {duration:.6}",
            stream.kind
        );
    }
    let opus = normalized_join.with_extension("opus");
    crate::encode_opus_sample(&normalized_join, &opus)
        .await
        .unwrap();
    struct Discard;
    impl crate::AudioSink for Discard {
        fn write(&mut self, _: &[i16]) -> Result<(), String> {
            Ok(())
        }
    }
    let options = crate::AudioDecodeOptions {
        sample_rate: 16_000,
    };
    let expected = crate::decode_audio(&normalized_join, options, Discard)
        .await
        .unwrap();
    let actual = crate::decode_audio(&opus, options, Discard).await.unwrap();
    assert_eq!(
        actual.samples, expected.samples,
        "Opus must retain all normalized audio samples"
    );
    assert_clean(directory.path());

    // Explicit opt-in keeps binary fixtures out of Git and normal test runs.
    if let Some(root) = std::env::var_os("BSR_COPY_CONCAT_REPRO_DIR") {
        let output = PathBuf::from(root).join(rate.to_string());
        fs::create_dir_all(&output).unwrap();
        for entry in fs::read_dir(directory.path()).unwrap() {
            let path = entry.unwrap().path();
            if matches!(
                path.extension().and_then(|ext| ext.to_str()),
                Some("ts" | "mp4" | "opus")
            ) {
                fs::copy(&path, output.join(path.file_name().unwrap())).unwrap();
            }
        }
        for prefix in ["", "normalized-"] {
            let list: String = (0..3)
                .map(|index| {
                    let name = if prefix.is_empty() {
                        format!("cut-{index}.mp4")
                    } else {
                        format!("normalized-{index}.mp4")
                    };
                    file_entry(Path::new(&name))
                })
                .collect();
            fs::write(output.join(format!("{prefix}list.txt")), list).unwrap();
        }
        eprintln!("Retained copy-concat fixtures: {}", output.display());
    }
}

fn coded_nals<'a>(packet: &'a CodedPacket, extradata: &[u8]) -> Vec<&'a [u8]> {
    let length_size = usize::from((extradata[4] & 3) + 1);
    let mut bytes = packet.payload.as_slice();
    let mut nals = Vec::new();
    while !bytes.is_empty() {
        assert!(bytes.len() >= length_size);
        let length = bytes[..length_size]
            .iter()
            .fold(0usize, |value, byte| (value << 8) | usize::from(*byte));
        bytes = &bytes[length_size..];
        assert!(length > 0 && length <= bytes.len());
        let nal = &bytes[..length];
        if !matches!(nal[0] & 31, 7 | 8) {
            nals.push(nal);
        }
        bytes = &bytes[length..];
    }
    nals
}

#[tokio::test]
async fn native_concat_copies_both_files_in_order_without_a_cli() {
    let directory = tempfile::tempdir().unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/video_frame/fixtures/seek.mp4");
    let list = directory.path().join("list.txt");
    fs::write(&list, file_entry(&source).repeat(2)).unwrap();
    let output = directory.path().join("joined.mp4");
    execute_media_plan(concat_plan(&list, &output), None)
        .await
        .unwrap();
    let expected = snapshot(&source);
    let actual = snapshot(&output);
    assert_eq!(actual.streams.len(), expected.streams.len());
    for (actual, expected) in actual.streams.iter().zip(&expected.streams) {
        assert_eq!(actual.codec, expected.codec);
        assert_eq!(actual.packets.len(), expected.packets.len() * 2);
        for (index, (actual_packet, expected_packet)) in actual
            .packets
            .iter()
            .zip(expected.packets.iter().cycle())
            .enumerate()
        {
            // The concat demuxer inserts SPS/PPS through h264_mp4toannexb.
            // Check every remaining coded NAL instead of mistaking this
            // required container conversion for a payload corruption.
            assert!(
                coded_nals(actual_packet, &actual.extradata)
                    == coded_nals(expected_packet, &expected.extradata),
                "coded packet {index} changed"
            );
        }
    }
    assert_clean(directory.path());
}

#[tokio::test]
async fn native_concat_matches_cli_packets_duration_hints_and_ts_mux_delay() {
    if !has_cli() {
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("片段 [one]'s.mp4");
    synthetic(&source);
    for hints in [false, true] {
        let list = directory.path().join("list.txt");
        let mut content = file_entry(&source);
        if hints {
            content.push_str("duration 1.500000\n");
        }
        content.push_str(&file_entry(&source));
        fs::write(&list, content).unwrap();
        for extension in ["mp4", "ts"] {
            let output = directory.path().join(format!("native.{extension}"));
            let oracle = directory.path().join(format!("oracle.{extension}"));
            execute_media_plan(concat_plan(&list, &output), None)
                .await
                .unwrap();
            cli(
                &strings(&[
                    "-f",
                    "concat",
                    "-safe",
                    "0",
                    "-i",
                    list.to_str().unwrap(),
                    "-c",
                    "copy",
                ]),
                &oracle,
            );
            assert_movie(
                &snapshot(&output),
                &snapshot(&oracle),
                &format!("concat {extension} duration hints={hints}"),
            );
        }
    }
    assert_clean(directory.path());
}

#[tokio::test]
async fn unsupported_native_input_formats_and_options_preserve_existing_output() {
    let directory = tempfile::tempdir().unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/video_frame/fixtures/seek.mp4");
    let output = directory.path().join("kept.mp4");
    fs::write(&output, b"previous media").unwrap();
    for (format, options) in [
        (Some("bsr_missing_demuxer"), BTreeMap::new()),
        (
            Some("mov"),
            [("bsr_unused_input_option".into(), "1".into())]
                .into_iter()
                .collect(),
        ),
        (Some("mov\0invalid"), BTreeMap::new()),
        (
            None,
            [("bsr_option".into(), "value\0invalid".into())]
                .into_iter()
                .collect(),
        ),
    ] {
        let mut request = plan(&source, &output, StreamMode::Copy, StreamMode::Copy);
        request.inputs[0].format = format.map(str::to_owned);
        request.inputs[0].options = options;
        assert!(execute_media_plan(request, None).await.is_err());
        assert_eq!(fs::read(&output).unwrap(), b"previous media");
        assert_clean(directory.path());
    }
}

#[tokio::test]
async fn cancelled_concat_retains_its_list_until_the_native_input_closes() {
    use std::{
        io::Write,
        sync::Weak,
        time::{Duration, Instant},
    };
    let retained = Arc::new(Mutex::new(None::<(PathBuf, Weak<tempfile::TempPath>)>));
    let observed = retained.clone();
    crate::audio::opus_tests::assert_cancelled_media(
        move |url, output| async move {
            let mut list = tempfile::Builder::new()
                .prefix(".retained-list-")
                .tempfile_in(output.parent().unwrap())
                .unwrap();
            list.write_all(file_entry(&url).as_bytes()).unwrap();
            list.flush().unwrap();
            let mut input = MediaInput::owned_file(list.into_temp_path());
            input.format = Some("concat".into());
            input.options.insert("safe".into(), "0".into());
            input
                .options
                .insert("protocol_whitelist".into(), "file,http,tcp".into());
            *observed.lock().unwrap() = Some((
                input.file_path().unwrap().to_path_buf(),
                Arc::downgrade(input._lease.as_ref().unwrap()),
            ));
            let request = MediaPlan {
                inputs: vec![input],
                outputs: vec![MediaOutput::file(output)],
                routes: vec![StreamRoute {
                    source: StreamSource::Input {
                        input: 0,
                        selector: StreamSelector::FirstAudio,
                    },
                    output: 0,

                    required: true,
                    mode: StreamMode::Copy,
                }],
                ..Default::default()
            };
            execute_media_plan(request, None)
                .await
                .map(|_| ())
                .map_err(|e| e.to_string())
        },
        "wav",
        ".media-job-",
    )
    .await;
    let (path, lease) = retained.lock().unwrap().take().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while lease.upgrade().is_some() {
        assert!(
            Instant::now() < deadline,
            "cancelled concat worker retained its list"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(!path.exists());
}
