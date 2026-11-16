//! Resolve missing SAR before format comparison and filter submission.
use ffmpeg_next::{frame, Rational};

/// One instance per decoded video stream, never shared across media inputs.
pub(super) struct VideoSar(Rational);

impl Default for VideoSar {
    fn default() -> Self {
        Self(Rational(1, 1))
    }
}

impl VideoSar {
    pub(super) fn normalize(&mut self, frame: &mut frame::Video) {
        let sar = frame.aspect_ratio();
        if sar.numerator() > 0 && sar.denominator() > 0 {
            self.0 = sar;
        } else {
            // Renewed HLS streams may omit SAR in their SPS. Retain the last
            // explicit value in this input, or square pixels at its beginning.
            // Do not seed from container metadata: it can describe a later
            // fragment rather than the first decoded frame.
            unsafe {
                (*frame.as_mut_ptr()).sample_aspect_ratio = self.0.into();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn normalize(state: &mut VideoSar, sar: Rational) -> Rational {
        let mut frame = frame::Video::empty();
        unsafe {
            (*frame.as_mut_ptr()).sample_aspect_ratio = sar.into();
        }
        state.normalize(&mut frame);
        frame.aspect_ratio()
    }

    #[test]
    fn missing_sar_defaults_to_square_pixels() {
        let mut state = VideoSar::default();
        for sar in [Rational(0, 1), Rational(0, 0), Rational(1, 0)] {
            assert_eq!(normalize(&mut state, sar), Rational(1, 1));
        }
    }

    #[test]
    fn missing_sar_inherits_the_latest_explicit_value() {
        let mut state = VideoSar::default();
        for explicit in [Rational(1, 1), Rational(4, 3), Rational(2, 1)] {
            assert_eq!(normalize(&mut state, explicit), explicit);
            for _ in 0..3 {
                assert_eq!(normalize(&mut state, Rational(0, 1)), explicit);
            }
        }
    }

    #[test]
    fn explicit_sar_after_missing_frames_is_not_overridden() {
        let mut state = VideoSar::default();
        assert_eq!(normalize(&mut state, Rational(0, 1)), Rational(1, 1));
        assert_eq!(normalize(&mut state, Rational(4, 3)), Rational(4, 3));
    }

    #[test]
    fn separate_inputs_do_not_inherit_each_others_sar() {
        let mut first = VideoSar::default();
        let mut second = VideoSar::default();
        normalize(&mut first, Rational(4, 3));
        assert_eq!(normalize(&mut second, Rational(0, 1)), Rational(1, 1));
        assert_eq!(normalize(&mut first, Rational(0, 1)), Rational(4, 3));
    }
}
