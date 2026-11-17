use super::opus_tests::{assert_equivalent, cli, has_cli};
use super::*;
use std::{fs, path::Path};

#[derive(Default)]
struct Samples(Vec<i16>);
impl AudioSink for Samples {
    fn write(&mut self, samples: &[i16]) -> Result<(), String> {
        self.0.extend_from_slice(samples);
        Ok(())
    }
}

pub(super) fn delayed_gapped_audio(path: &Path) {
    cli(
        &[
            "-f",
            "lavfi",
            "-i",
            "color=s=64x64:r=30:d=4",
            "-f",
            "lavfi",
            "-i",
            "sine=sample_rate=44100:duration=3",
            "-filter_complex",
            "[1:a]asetpts=PTS+0.25/TB+gte(T\\,1)*0.4/TB[a]",
            "-map",
            "0:v",
            "-map",
            "[a]",
            "-c:v",
            "libx264",
            "-threads:v",
            "1",
            "-c:a",
            "aac",
        ],
        path,
    );
}

#[tokio::test]
async fn playback_pcm_retains_leading_delay_and_internal_gaps_for_subtitles() {
    if !has_cli() {
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("delayed-gap.mp4");
    delayed_gapped_audio(&input);
    for range in [
        None,
        Some(AudioRange {
            start: 1.35,
            duration: 0.6,
        }),
    ] {
        let decoded = decode_audio_timeline(
            &input,
            AudioDecodeOptions {
                sample_rate: 16_000,
            },
            range,
            Samples::default(),
        )
        .await
        .unwrap();
        let oracle = directory.path().join("oracle.pcm");
        let mut args = vec!["-i".to_string(), input.to_str().unwrap().to_string()];
        if let Some(range) = range {
            args.splice(0..0, ["-ss".into(), range.start.to_string()]);
            args.extend(["-t".into(), range.duration.to_string()]);
        }
        args.extend(
            [
                "-vn",
                "-af",
                "aresample=16000:async=1:min_hard_comp=0.001:first_pts=0",
                "-ac",
                "1",
                "-ar",
                "16000",
                "-f",
                "s16le",
            ]
            .map(str::to_string),
        );
        cli(
            &args.iter().map(String::as_str).collect::<Vec<_>>(),
            &oracle,
        );
        let actual: Vec<u8> = decoded
            .sink
            .0
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect();
        let expected = fs::read(oracle).unwrap();
        assert!(
            actual == expected,
            "range={range:?}: lengths {} vs {}, first differing byte {:?}",
            actual.len(),
            expected.len(),
            actual.iter().zip(&expected).position(|(a, b)| a != b)
        );
        if range.is_none() {
            assert!(decoded.sink.0[..1600].iter().all(|sample| *sample == 0));
            assert!(decoded.sink.0[23_200..24_800]
                .iter()
                .all(|sample| *sample == 0));
            assert!((decoded.samples as f64 / 16_000.0 - 3.65).abs() < 0.03);
        }
    }
}

#[tokio::test]
async fn playback_opus_profiles_match_cli_and_pcm_timeline() {
    if !has_cli() {
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("delayed-gap.mp4");
    delayed_gapped_audio(&input);
    let decoded = decode_audio_timeline(
        &input,
        AudioDecodeOptions {
            sample_rate: 16_000,
        },
        None,
        Samples::default(),
    )
    .await
    .unwrap();
    for upload in [false, true] {
        let output = directory.path().join("native.opus");
        if upload {
            encode_opus_upload_timeline(&input, &output).await.unwrap();
        } else {
            encode_opus_sample_timeline(&input, &output).await.unwrap();
        }
        let oracle = directory.path().join("oracle.opus");
        let mut args = vec![
            "-i",
            input.to_str().unwrap(),
            "-vn",
            "-af",
            "aresample=async=1:min_hard_comp=0.001:first_pts=0",
            "-c:a",
            "libopus",
            "-b:a",
            if upload { "128k" } else { "64k" },
            "-vbr",
            "on",
            "-compression_level",
            "10",
        ];
        if !upload {
            args.extend(["-ar", "16000", "-ac", "1"]);
        }
        cli(&args, &oracle);
        assert_equivalent(&output, &oracle, "playback clock", 1);
        let opus = decode_audio(
            &output,
            AudioDecodeOptions {
                sample_rate: 16_000,
            },
            Samples::default(),
        )
        .await
        .unwrap();
        assert!(opus.samples.abs_diff(decoded.samples) <= 1);
    }
}

#[tokio::test]
async fn playback_audio_cancellation_preserves_existing_output() {
    opus_tests::assert_cancelled_opus(|input, output| async move {
        encode_opus_sample_timeline(&input, &output).await
    })
    .await;
}
