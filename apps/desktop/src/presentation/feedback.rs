use ultratasks_core::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputArea {
    Title,
    Schedule,
    Form,
}

pub struct Validation {
    pub area: InputArea,
    pub message: String,
}
impl Validation {
    pub fn from_error(error: &Error, all_day: bool) -> Self {
        let area = match error {
            Error::InvalidInput {
                field: Field::Title,
                ..
            } => InputArea::Title,
            Error::InvalidInput {
                field: Field::Date | Field::LocalTime | Field::Instant | Field::Range,
                ..
            }
            | Error::AmbiguousLocalTime { .. }
            | Error::NonexistentLocalTime { .. } => InputArea::Schedule,
            _ => InputArea::Form,
        };
        let message = if all_day
            && matches!(
                error,
                Error::InvalidInput {
                    problem: InputProblem::NonIncreasingRange,
                    ..
                }
            ) {
            "最終日は開始日以降にしてください".into()
        } else {
            user_error(error)
        };
        Self { area, message }
    }
}

pub fn user_error(error: &Error) -> String {
    use Error::*;
    match error {
        InvalidInput {
            problem: InputProblem::NonIncreasingRange,
            ..
        } => "終了は開始より後にしてください",
        InvalidInput {
            field: Field::Title,
            problem,
        } => match problem {
            InputProblem::Empty => "タイトルを入力してください",
            InputProblem::TooLong => "タイトルは240文字以内で入力してください",
            InputProblem::ControlCharacter => "タイトルは改行や制御文字を含めずに入力してください",
            _ => "タイトルを確認してください",
        },
        InvalidInput {
            field: Field::Date, ..
        } => "日付は YYYY-MM-DD で、存在する日付を入力してください",
        InvalidInput {
            field: Field::LocalTime | Field::Instant,
            ..
        } => "時刻は 09:00 や 9時の形式で、0:00〜23:59を入力してください",
        InvalidInput {
            field: Field::Zone, ..
        } => "タイムゾーンを確認してください",
        InvalidInput {
            field: Field::Search,
            ..
        } => "検索語は128文字以内で入力してください",
        InvalidInput { .. } => "入力内容と指定範囲を確認してください",
        AmbiguousLocalTime { .. } => "この時刻は二度あります。早い時刻か遅い時刻を選んでください",
        NonexistentLocalTime { .. } => {
            "夏時間の切り替えにより存在しない時刻です。別の時刻を入力してください"
        }
        Conflict { .. } | HistoryConflict { .. } => {
            "別の更新があります。最新の内容を確認してください"
        }
        NotFound(_) => "対象が見つかりません。削除されていないか確認してください",
        AlreadyExists(_) => "同じ項目が既に保存されています。最新の内容を開いてください",
        NotEmpty(_) => "使用中の所属先は削除できません。項目を移動してから削除してください",
        ProtectedCollection(_) => "既定の所属先は削除できません",
        AlreadyOpen => "この保存先は別のウィンドウで開かれています",
        IdUnavailable => "項目を作成できませんでした。もう一度お試しください",
        TimeBlockTaskMismatch { .. } => {
            "作業枠の関連タスクが一致しません。最新の内容を開いてください"
        }
        LimitExceeded {
            kind: LimitKind::BlocksPerTask,
            ..
        } => "一つのタスクに設定できる作業枠の上限に達しました",
        LimitExceeded { .. } => "一度に処理できる上限を超えました。対象を減らしてください",
        History(_) => "この履歴は操作できません。最新の状態を確認してください",
        StalePage | CursorMismatch => "表示内容が更新されています。再読み込みしてください",
        QueryBudgetExceeded => "検索の処理上限を超えました。範囲を絞ってください",
        TimeDataUnavailable => "日時の情報を読み込めませんでした",
        Storage {
            kind: StorageKind::NoSpace,
            ..
        } => "保存先の空き容量が不足しています",
        Storage {
            kind: StorageKind::PermissionDenied,
            ..
        } => "保存先へのアクセス権限を確認してください",
        Storage {
            kind: StorageKind::Busy,
            ..
        } => "保存先が使用中です。少し待ってから再試行してください",
        Storage {
            kind: StorageKind::Corrupt,
            ..
        } => "保存先を読み込めませんでした。データの状態を確認してください",
        Storage {
            kind: StorageKind::MultipleLinks,
            ..
        } => "複数のハードリンクを持つデータベースは開けません",
        Storage { .. } => "保存先の読み書きに失敗しました。接続と空き容量を確認してください",
        UnsupportedSchema { .. } => "この保存先には新しいバージョンのアプリが必要です",
        RecoveryRequired => "保存結果の確認のため、保存先を開き直してください",
        FixtureRequiresEmptyDatabase => "デモデータは空の保存先にだけ作成できます",
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validation_is_localized_and_assigned_to_inputs() {
        let reversed = Error::InvalidInput {
            field: Field::Range,
            problem: InputProblem::NonIncreasingRange,
        };
        let timed = Validation::from_error(&reversed, false);
        assert_eq!(timed.area, InputArea::Schedule);
        assert_eq!(timed.message, "終了は開始より後にしてください");
        assert_eq!(
            Validation::from_error(&reversed, true).message,
            "最終日は開始日以降にしてください"
        );
        let storage = user_error(&Error::Storage {
            stage: StorageStage::Write,
            kind: StorageKind::Io,
            outcome: CommitKnowledge::NotCommitted,
        });
        assert!(!storage.contains("Storage"));
        assert!(!storage.contains("SQL"));
        assert_eq!(
            crate::worker::Failure::Core(reversed).to_string(),
            "終了は開始より後にしてください"
        );
        assert_eq!(
            crate::worker::Failure::Core(Error::Storage {
                stage: StorageStage::Open,
                kind: StorageKind::MultipleLinks,
                outcome: CommitKnowledge::NotCommitted,
            })
            .to_string(),
            "複数のハードリンクを持つデータベースは開けません"
        );
    }
}
