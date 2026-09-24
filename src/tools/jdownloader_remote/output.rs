use std::fmt;
use std::io::Write;

use anyhow::Result;
use clap::ValueEnum;
use serde::Serialize;

const HEADER_PADDING: usize = 2;

#[derive(Clone, Copy, ValueEnum)]
pub enum Format {
    Text,
    Table,
    Json,
}

pub enum Cell {
    Text(String),
    Int(i64),
    Bool(bool),
    Empty,
}

impl Cell {
    pub fn int(value: impl Into<i64>) -> Self {
        Self::Int(value.into())
    }
}

impl From<String> for Cell {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for Cell {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

impl From<i64> for Cell {
    fn from(value: i64) -> Self {
        Self::Int(value)
    }
}

impl From<bool> for Cell {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl<T: Into<Cell>> From<Option<T>> for Cell {
    fn from(value: Option<T>) -> Self {
        value.map_or(Self::Empty, Into::into)
    }
}

impl fmt::Display for Cell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Cell::Text(text) => f.write_str(text),
            Cell::Int(value) => write!(f, "{value}"),
            Cell::Bool(true) => f.write_str("True"),
            Cell::Bool(false) => f.write_str("False"),
            Cell::Empty => Ok(()),
        }
    }
}

pub struct Table {
    headers: &'static [&'static str],
    rows: Vec<Vec<Cell>>,
    text_columns: usize,
}

impl Table {
    pub fn new(headers: &'static [&'static str], rows: Vec<Vec<Cell>>) -> Self {
        Self {
            headers,
            rows,
            text_columns: headers.len(),
        }
    }

    pub fn text_columns(self, text_columns: usize) -> Self {
        Self {
            text_columns,
            ..self
        }
    }

    fn write_text(&self, out: &mut impl Write) -> Result<()> {
        for row in &self.rows {
            let cells: Vec<String> = row
                .iter()
                .take(self.text_columns)
                .map(ToString::to_string)
                .collect();
            writeln!(out, "{}", cells.join("\t"))?;
        }
        Ok(())
    }

    fn write_table(&self, out: &mut impl Write) -> Result<()> {
        let numeric: Vec<bool> = (0..self.headers.len())
            .map(|column| {
                let mut cells = self
                    .rows
                    .iter()
                    .filter_map(|row| row.get(column))
                    .filter(|cell| !matches!(cell, Cell::Empty))
                    .peekable();
                cells.peek().is_some() && cells.all(|cell| matches!(cell, Cell::Int(_)))
            })
            .collect();
        let rows: Vec<Vec<String>> = self
            .rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|cell| cell.to_string().trim().to_owned())
                    .collect()
            })
            .collect();
        let widths: Vec<usize> = self
            .headers
            .iter()
            .enumerate()
            .map(|(column, header)| {
                rows.iter()
                    .filter_map(|row| row.get(column))
                    .map(|cell| cell.chars().count())
                    .chain([header.chars().count() + HEADER_PADDING])
                    .max()
                    .unwrap_or_default()
            })
            .collect();
        let rule: Vec<String> = widths.iter().map(|width| "-".repeat(*width)).collect();
        write_line(out, self.headers.iter().copied(), &widths, &numeric)?;
        write_line(out, rule.iter().map(String::as_str), &widths, &numeric)?;
        for row in &rows {
            write_line(out, row.iter().map(String::as_str), &widths, &numeric)?;
        }
        Ok(())
    }
}

fn write_line<'a>(
    out: &mut impl Write,
    cells: impl Iterator<Item = &'a str>,
    widths: &[usize],
    numeric: &[bool],
) -> Result<()> {
    let cells: Vec<String> = cells
        .zip(widths.iter().zip(numeric))
        .map(|(cell, (&width, &numeric))| {
            if numeric {
                format!("{cell:>width$}")
            } else {
                format!("{cell:<width$}")
            }
        })
        .collect();
    writeln!(out, "{}", cells.join("  ").trim_end())?;
    Ok(())
}

pub fn write_items<T: Serialize>(
    out: &mut impl Write,
    format: Format,
    items: &[T],
    table: Table,
) -> Result<()> {
    match format {
        Format::Json => write_json(out, items),
        Format::Table => table.write_table(out),
        Format::Text => table.write_text(out),
    }
}

pub fn write_json<T: Serialize + ?Sized>(out: &mut impl Write, value: &T) -> Result<()> {
    let value = serde_json::to_value(value)?;
    serde_json::to_writer_pretty(&mut *out, &value)?;
    writeln!(out)?;
    Ok(())
}
