/// OSの軽減設定はアプリ内の選択より優先する。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum MotionMode {
    #[default]
    System,
    Reduced,
    None,
}

/// 描画側は0msを即時完了として扱い、完了後のframeを予約しない。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MotionSpec {
    pub fade_ms: u32,
    pub move_ms: u32,
}

pub fn motion(mode: MotionMode, os_reduced: bool) -> MotionSpec {
    if os_reduced || mode != MotionMode::System {
        MotionSpec {
            fade_ms: 0,
            move_ms: 0,
        }
    } else {
        MotionSpec {
            fade_ms: 150,
            move_ms: 200,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_motion_is_short_and_finite() {
        assert_eq!(
            motion(MotionMode::System, false),
            MotionSpec {
                fade_ms: 150,
                move_ms: 200,
            }
        );
    }

    #[test]
    fn os_and_app_reductions_are_immediate() {
        for mode in [MotionMode::System, MotionMode::Reduced, MotionMode::None] {
            for os_reduced in [false, true] {
                if mode != MotionMode::System || os_reduced {
                    assert_eq!(
                        motion(mode, os_reduced),
                        MotionSpec {
                            fade_ms: 0,
                            move_ms: 0,
                        },
                        "{mode:?}, os_reduced={os_reduced}"
                    );
                }
            }
        }
    }
}
