/// Shared work-item status vocabulary for progress rows.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum WorkItemStatus {
    /// Work item finished all required work.
    Completed,
    /// Work item finished its main search and is computing final outputs.
    Finishing,
    /// Work item is paused.
    Paused,
    /// Work item is ready to run when capacity is available.
    Queued,
    /// Work item finished with an invalid or rejected result.
    Rejected,
    /// Work item is actively running.
    Running,
    /// Work item has not started.
    Waiting,
}

const WORK_ITEM_STATUS_RUNNING_SORT_RANK: usize = 0;
const WORK_ITEM_STATUS_FINISHING_SORT_RANK: usize = 1;
const WORK_ITEM_STATUS_QUEUED_SORT_RANK: usize = 2;
const WORK_ITEM_STATUS_PAUSED_SORT_RANK: usize = 3;
const WORK_ITEM_STATUS_WAITING_SORT_RANK: usize = 4;
const WORK_ITEM_STATUS_UNKNOWN_ACTIVE_SORT_RANK: usize = WORK_ITEM_STATUS_WAITING_SORT_RANK;
const WORK_ITEM_STATUS_REJECTED_SORT_RANK: usize = 5;
const WORK_ITEM_STATUS_COMPLETED_SORT_RANK: usize = 6;

impl WorkItemStatus {
    /// Stable label used in progress contracts and terminal display.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Finishing => "finishing",
            Self::Paused => "paused",
            Self::Queued => "queued",
            Self::Rejected => "rejected",
            Self::Running => "running",
            Self::Waiting => "waiting",
        }
    }

    /// Parse a stable work-item status label.
    pub fn from_label(label: &str) -> Option<Self> {
        match label {
            label if label == Self::Completed.as_str() => Some(Self::Completed),
            label if label == Self::Finishing.as_str() => Some(Self::Finishing),
            label if label == Self::Paused.as_str() => Some(Self::Paused),
            label if label == Self::Queued.as_str() => Some(Self::Queued),
            label if label == Self::Rejected.as_str() => Some(Self::Rejected),
            label if label == Self::Running.as_str() => Some(Self::Running),
            label if label == Self::Waiting.as_str() => Some(Self::Waiting),
            _ => None,
        }
    }

    /// Returns whether a work-item status label means the row is complete.
    pub fn label_is_completed(label: &str) -> bool {
        Self::from_label(label) == Some(Self::Completed)
    }

    /// Sort rank used by progress views to keep active work above terminal rows.
    pub fn label_sort_rank(label: &str) -> usize {
        Self::from_label(label).map_or(WORK_ITEM_STATUS_UNKNOWN_ACTIVE_SORT_RANK, Self::sort_rank)
    }

    const fn sort_rank(self) -> usize {
        match self {
            Self::Running => WORK_ITEM_STATUS_RUNNING_SORT_RANK,
            Self::Finishing => WORK_ITEM_STATUS_FINISHING_SORT_RANK,
            Self::Queued => WORK_ITEM_STATUS_QUEUED_SORT_RANK,
            Self::Paused => WORK_ITEM_STATUS_PAUSED_SORT_RANK,
            Self::Waiting => WORK_ITEM_STATUS_WAITING_SORT_RANK,
            Self::Rejected => WORK_ITEM_STATUS_REJECTED_SORT_RANK,
            Self::Completed => WORK_ITEM_STATUS_COMPLETED_SORT_RANK,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::WorkItemStatus;

    #[test]
    fn finishing_status_is_active_and_not_completed() {
        assert_eq!(WorkItemStatus::Finishing.as_str(), "finishing");
        assert!(!WorkItemStatus::label_is_completed(
            WorkItemStatus::Finishing.as_str()
        ));
        assert!(
            WorkItemStatus::label_sort_rank(WorkItemStatus::Finishing.as_str())
                < WorkItemStatus::label_sort_rank(WorkItemStatus::Completed.as_str())
        );
    }
}
