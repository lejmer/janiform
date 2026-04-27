use super::display::truncate_for_table;

const CONTEXT_WORK_ITEM_COLUMN_MAX_WIDTH: usize = 18;
const CONTEXT_WORK_ITEM_COLUMN_MIN_WIDTH: usize = 8;
const WORK_ITEM_ELAPSED_COLUMN_WIDTH: usize = 10;
const WORK_ITEM_LABEL_COLUMN_WIDTH: usize = 34;
const WORK_ITEM_PROGRESS_COLUMN_WIDTH: usize = 26;
const WORK_ITEM_STATUS_COLUMN_WIDTH: usize = 10;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct WorkItemTableColumn {
    pub(super) key: String,
    pub(super) label: String,
    pub(super) width: usize,
}

impl WorkItemTableColumn {
    pub(super) fn new(key: impl Into<String>, label: impl Into<String>, width: usize) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            width,
        }
    }
}

pub(super) fn default_work_item_table_columns() -> Vec<WorkItemTableColumn> {
    vec![
        WorkItemTableColumn::new("label", "Work item", WORK_ITEM_LABEL_COLUMN_WIDTH),
        WorkItemTableColumn::new("progress", "Progress", WORK_ITEM_PROGRESS_COLUMN_WIDTH),
        WorkItemTableColumn::new("elapsed", "Elapsed", WORK_ITEM_ELAPSED_COLUMN_WIDTH),
        WorkItemTableColumn::new("status", "Status", WORK_ITEM_STATUS_COLUMN_WIDTH),
    ]
}

pub(super) fn context_work_item_column_width(label_width: usize, value_width: usize) -> usize {
    label_width.max(value_width).clamp(
        CONTEXT_WORK_ITEM_COLUMN_MIN_WIDTH,
        CONTEXT_WORK_ITEM_COLUMN_MAX_WIDTH,
    )
}

pub(super) fn format_work_item_table_row(
    columns: &[WorkItemTableColumn],
    values: &[&str],
) -> String {
    let mut row = String::from("   ");

    for (column_index, column) in columns.iter().enumerate() {
        if column_index > 0 {
            row.push(' ');
        }

        let value = values.get(column_index).copied().unwrap_or("-");
        let value = truncate_for_table(value, column.width);

        let formatted_value = format!("{value:<width$}", width = column.width);

        row.push_str(formatted_value.as_str());
    }

    row
}
