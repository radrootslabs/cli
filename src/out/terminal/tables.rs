#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalTableColumn {
    pub label: String,
    pub min_width: usize,
    pub max_width: usize,
}

impl TerminalTableColumn {
    pub fn new(label: impl Into<String>, min_width: usize, max_width: usize) -> Self {
        Self {
            label: label.into(),
            min_width,
            max_width: max_width.max(min_width),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalTableRow {
    pub cells: Vec<String>,
}

impl TerminalTableRow {
    pub fn new(cells: Vec<impl Into<String>>) -> Self {
        Self {
            cells: cells.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalTable {
    pub columns: Vec<TerminalTableColumn>,
    pub rows: Vec<TerminalTableRow>,
    pub empty: Option<String>,
}

impl TerminalTable {
    pub fn new(columns: Vec<TerminalTableColumn>) -> Self {
        Self {
            columns,
            rows: Vec::new(),
            empty: None,
        }
    }

    pub fn with_row(mut self, row: TerminalTableRow) -> Self {
        self.rows.push(row);
        self
    }

    pub fn with_empty(mut self, empty: impl Into<String>) -> Self {
        self.empty = Some(empty.into());
        self
    }

    pub fn column_widths(&self, width: usize) -> Vec<usize> {
        if self.columns.is_empty() {
            return Vec::new();
        }
        let separators = self.columns.len().saturating_sub(1) * 2;
        let available = width.saturating_sub(separators).max(self.columns.len());
        let mut widths = self
            .columns
            .iter()
            .map(|column| column.min_width.min(column.max_width))
            .collect::<Vec<_>>();
        let min_total = widths.iter().sum::<usize>();
        let mut remaining = available.saturating_sub(min_total);
        for (index, column) in self.columns.iter().enumerate() {
            if remaining == 0 {
                break;
            }
            let current = widths[index];
            let target = column.max_width.max(current);
            let add = target.saturating_sub(current).min(remaining);
            widths[index] += add;
            remaining -= add;
        }
        widths
    }
}

pub fn truncate_cell(value: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let chars = value.chars().collect::<Vec<_>>();
    if chars.len() <= width {
        return value.to_owned();
    }
    if width == 1 {
        return "…".to_owned();
    }
    chars
        .into_iter()
        .take(width.saturating_sub(1))
        .collect::<String>()
        + "…"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_cells_deterministically() {
        assert_eq!(truncate_cell("abcdef", 4), "abc…");
        assert_eq!(truncate_cell("abcdef", 1), "…");
        assert_eq!(truncate_cell("abc", 4), "abc");
    }

    #[test]
    fn computes_stable_column_widths() {
        let table = TerminalTable::new(vec![
            TerminalTableColumn::new("Name", 4, 8),
            TerminalTableColumn::new("State", 5, 10),
        ]);

        assert_eq!(table.column_widths(20), vec![8, 10]);
        assert_eq!(table.column_widths(10), vec![4, 5]);
    }
}
