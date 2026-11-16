//! Discard exact audio retransmissions at MPEG-TS/HLS renewal boundaries.
//! Run on original demux packets, before timestamps are rebased or clamped.
use ffmpeg_next::{ffi, format, media, Packet, Rational};
use std::collections::VecDeque;

const MAX_BYTES: usize = 4 * 1024 * 1024;
const MAX_PACKETS: usize = 2048;
const WINDOW_SECONDS: i64 = 30;

pub(super) struct AudioReplay {
    streams: Vec<Option<History>>,
}

impl AudioReplay {
    pub(super) fn new(input: &format::context::Input) -> Self {
        // Do not change the semantics of ordinary file copying. TS/HLS can
        // retransmit previously delivered media when a live URL is renewed.
        let discontinuous =
            unsafe { (*input.format().as_ptr()).flags & ffi::AVFMT_TS_DISCONT != 0 };
        Self {
            streams: input
                .streams()
                .map(|stream| {
                    let base = stream.time_base();
                    (discontinuous
                        && stream.parameters().medium() == media::Type::Audio
                        && base.numerator() > 0
                        && base.denominator() > 0)
                        .then(|| {
                            let window = unsafe {
                                ffi::av_rescale_q(
                                    WINDOW_SECONDS,
                                    Rational(1, 1).into(),
                                    base.into(),
                                )
                            };
                            History::new(stream.index(), window)
                        })
                })
                .collect(),
        }
    }

    pub(super) fn is_replay(&mut self, packet: &Packet) -> bool {
        self.streams
            .get_mut(packet.stream())
            .and_then(Option::as_mut)
            .is_some_and(|history| history.is_replay(packet))
    }
}

struct History {
    stream: usize,
    window: i64,
    packets: VecDeque<(Packet, usize)>,
    bytes: usize,
    discarded: u64,
}

impl History {
    fn new(stream: usize, window: i64) -> Self {
        Self {
            stream,
            window,
            packets: VecDeque::new(),
            bytes: 0,
            discarded: 0,
        }
    }

    fn clear(&mut self) {
        self.packets.clear();
        self.bytes = 0;
    }

    fn is_replay(&mut self, packet: &Packet) -> bool {
        let (Some(dts), Some(_)) = (packet.dts(), packet.pts()) else {
            self.clear();
            return false;
        };
        let bytes = packet.side_data().fold(packet.size(), |size, side| {
            size.saturating_add(side.data().len())
        });
        if packet.size() == 0 || bytes > MAX_BYTES {
            self.clear();
            return false;
        }
        if self
            .packets
            .back()
            .is_some_and(|(last, _)| dts <= last.dts().unwrap())
        {
            // Timestamp overlap alone is not enough: retain new content and
            // decoder configuration, even when the source clock runs backward.
            if self.packets.iter().any(|(previous, _)| {
                previous.dts() == packet.dts()
                    && previous.pts() == packet.pts()
                    && previous.duration() == packet.duration()
                    && previous.flags() == packet.flags()
                    && previous.data() == packet.data()
                    && previous.side_data().count() == packet.side_data().count()
                    && previous
                        .side_data()
                        .zip(packet.side_data())
                        .all(|(a, b)| a.kind() == b.kind() && a.data() == b.data())
            }) {
                self.discarded += 1;
                return true;
            }
            // An unrecognized reset starts a new history. Never deduplicate
            // against an unrelated earlier timeline.
            self.clear();
        }
        while self.packets.front().is_some_and(|(first, _)| {
            dts.saturating_sub(first.dts().unwrap()) > self.window
                || self.bytes + bytes > MAX_BYTES
                || self.packets.len() >= MAX_PACKETS
        }) {
            self.bytes -= self.packets.pop_front().unwrap().1;
        }
        self.bytes += bytes;
        self.packets.push_back((packet.clone(), bytes));
        false
    }
}

impl Drop for History {
    fn drop(&mut self) {
        if self.discarded > 0 {
            log::info!(
                "Discarded {} exact audio replay packets in input stream {}",
                self.discarded,
                self.stream
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(time: i64, data: &[u8]) -> Packet {
        let mut packet = Packet::copy(data);
        packet.set_pts(Some(time));
        packet.set_dts(Some(time));
        packet.set_duration(1024);
        packet
    }

    #[test]
    fn removes_only_exact_replay_and_preserves_forward_repeated_sounds() {
        let mut history = History::new(0, 48_000 * WINDOW_SECONDS);
        for time in [0, 1024, 2048] {
            assert!(!history.is_replay(&packet(time, b"silence")));
        }
        for time in [0, 1024, 2048] {
            assert!(history.is_replay(&packet(time, b"silence")));
        }
        assert!(!history.is_replay(&packet(3072, b"silence")));
        assert_eq!(history.discarded, 3);
    }

    #[test]
    fn unknown_clock_resets_are_not_deleted_or_matched_to_old_history() {
        let mut history = History::new(0, 48_000 * WINDOW_SECONDS);
        assert!(!history.is_replay(&packet(0, b"old")));
        assert!(!history.is_replay(&packet(1024, b"same")));
        assert!(!history.is_replay(&packet(0, b"new")));
        assert!(!history.is_replay(&packet(1024, b"same")));
    }

    #[test]
    fn changed_timing_or_side_data_is_not_a_replay() {
        for change in 0..4 {
            let mut history = History::new(0, 48_000 * WINDOW_SECONDS);
            assert!(!history.is_replay(&packet(0, b"audio")));
            let mut changed = packet(0, b"audio");
            match change {
                0 => changed.set_pts(Some(1)),
                1 => changed.set_duration(960),
                2 => changed.set_flags(ffmpeg_next::packet::Flags::CORRUPT),
                _ => unsafe {
                    use ffmpeg_next::packet::Mut;
                    let side = ffi::av_packet_new_side_data(
                        changed.as_mut_ptr(),
                        ffi::AVPacketSideDataType::AV_PKT_DATA_NEW_EXTRADATA,
                        1,
                    );
                    assert!(!side.is_null());
                    *side = 1;
                },
            }
            assert!(!history.is_replay(&changed));
        }
    }

    #[test]
    fn missing_timestamps_and_separate_streams_do_not_deduplicate() {
        let mut replay = AudioReplay {
            streams: vec![Some(History::new(0, 100)), Some(History::new(1, 100))],
        };
        let mut first = packet(0, b"same");
        assert!(!replay.is_replay(&first));
        first.set_stream(1);
        assert!(!replay.is_replay(&first));
        first.set_pts(None);
        assert!(!replay.is_replay(&first));
        first.set_pts(Some(0));
        assert!(!replay.is_replay(&first));
    }

    #[test]
    fn history_is_bounded_by_time_packet_count_and_bytes() {
        let mut history = History::new(0, 1);
        history.is_replay(&packet(0, b"a"));
        history.is_replay(&packet(2, b"a"));
        assert_eq!(history.packets.len(), 1);
        assert!(!history.is_replay(&packet(0, b"a")));

        let mut history = History::new(0, i64::MAX);
        for time in 0..=MAX_PACKETS {
            history.is_replay(&packet(time as i64, b"a"));
        }
        assert_eq!(history.packets.len(), MAX_PACKETS);
        assert!(!history.is_replay(&packet(0, b"a")));

        let mut history = History::new(0, i64::MAX);
        let large = vec![0; MAX_BYTES / 2 + 1];
        history.is_replay(&packet(0, &large));
        history.is_replay(&packet(1, &large));
        assert_eq!(history.packets.len(), 1);
        assert_eq!(history.bytes, large.len());
        assert!(!history.is_replay(&packet(0, &large)));
    }
}
